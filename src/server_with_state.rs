use std::{ops::ControlFlow, sync::Arc};

use futures::future::BoxFuture;

#[cfg(feature = "tracing")]
use tracing::{debug, info};

use async_lsp::{
    ClientSocket, ErrorCode, LanguageServer, ResponseError, Result,
    lsp_types::{
        DidChangeConfigurationParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
        DidOpenTextDocumentParams, DidSaveTextDocumentParams, InitializeParams, InitializeResult,
        InitializedParams, SaveOptions, TextDocumentSyncCapability, TextDocumentSyncKind,
        TextDocumentSyncOptions, TextDocumentSyncSaveOptions, Url,
    },
};

use crate::{server_state::ServerState, server_trait::Server, text_utils::Encoding};

const POSITION_ENCODING_PREFERRED_ORDER: [Encoding; 3] = [
    // First, prefer to use UTF-8 encoding, since this will make all of
    // the conversions for the custom language server handlers zero-cost
    Encoding::UTF8,
    // Second, prefer to use UTF-32 encoding, since this is
    // practically zero-cost for anything that Ropey needs
    Encoding::UTF32,
    // Lastly, use the standard UTF-16 encoding, which is universally
    // terrible, but also universally supported by all LSP clients
    Encoding::UTF16,
];

macro_rules! implement_method {
    ($async_lsp_method:ident => $our_server_trait_method:ident @ $request_type:ty) => {
        fn $async_lsp_method(
            &mut self,
            mut params: <$request_type as crate::requests::Request>::Params,
        ) -> BoxFuture<
            'static,
            Result<<$request_type as crate::requests::Request>::Response, Self::Error>,
        > {
            let server = Arc::clone(&self.server);
            let state = self.state.clone();
            Box::pin(async move {
                // 1. Try to extract the URL from the params for document tracking
                let url: Option<Url> =
                    <$request_type as crate::requests::Request>::extract_url(&params);
                let mut ver: Option<i32> = None;

                // 2. If we got an URL, track the document version & call the "modify params" callback
                if let Some(url) = url.as_ref() {
                    if let Some(doc) = state.document(url) {
                        ver.replace(doc.version());
                        <$request_type as crate::requests::Request>::modify_params(
                            &state,
                            &doc,
                            &mut params,
                        );
                    }
                }

                // 3. Call the user-defined language server function
                let mut result = server
                    .$our_server_trait_method(state.clone(), params)
                    .await?;

                // 4. Check our document again, if we had one originally
                if let Some(url) = url.as_ref() {
                    if let Some(doc) = state.document(url) {
                        // 4a. If the version changed, our result is stale, and we should try again
                        if ver.is_some_and(|v| v != doc.version()) {
                            return Err(ResponseError::new(
                                ErrorCode::CONTENT_MODIFIED,
                                "document was modified during processing",
                            ));
                        }
                        // 4b. Version is not stale, run the final "modify response" callback
                        <$request_type as crate::requests::Request>::modify_response(
                            &state,
                            &doc,
                            &mut result,
                        );
                    }
                }

                Ok(result)
            })
        }
    };
}

macro_rules! implement_methods {
    ($($lsp_method:ident => $server_method:ident @ $request_type:ty),* $(,)?) => {
        $(
            implement_method!($lsp_method => $server_method @ $request_type);
        )*
    };
}

/**
    The low-level language server implementation that automatically
    manages documents and forwards requests to the underlying server.

    Supports incremental updates of documents where possible, falling
    back to other implementations whenever incremental updates fail.
*/
pub(crate) struct LanguageServerWithState<T: Server> {
    server: Arc<T>,
    state: ServerState,
}

impl<T: Server> LanguageServerWithState<T> {
    pub(crate) fn new(client: ClientSocket, server: T) -> Self {
        let server = Arc::new(server);
        let state = ServerState::new::<T>(client);
        Self { server, state }
    }
}

impl<T: Server + Send + Sync + 'static> LanguageServer for LanguageServerWithState<T> {
    type Error = ResponseError;
    type NotifyResult = ControlFlow<async_lsp::Result<()>>;

    fn initialize(
        &mut self,
        params: InitializeParams,
    ) -> BoxFuture<'static, Result<InitializeResult, Self::Error>> {
        // 1. Extract available client position encodings, if any
        let client_position_encodings = params
            .capabilities
            .general
            .as_ref()
            .and_then(|g| g.position_encodings.clone())
            .filter(|e| !e.is_empty());

        // 2. Get server info & capabilities from the server implementor
        let mut result = InitializeResult {
            server_info: T::server_info(),
            capabilities: T::server_capabilities(params.capabilities).unwrap_or_default(),
        };

        // 3. Try to figure out what position encoding best matches what
        //    both our server + the connected client prefers / supports
        let mut negotiated_position_encoding = Encoding::default();
        if let Some(client_available_encodings) = client_position_encodings {
            let client_available_encodings: Vec<Encoding> = client_available_encodings
                .into_iter()
                .map(Into::into)
                .collect();
            for server_preferred_encoding in POSITION_ENCODING_PREFERRED_ORDER {
                if client_available_encodings.contains(&server_preferred_encoding) {
                    negotiated_position_encoding = server_preferred_encoding;
                    break;
                }
            }
        }

        // 4. Insert capabilities for our automatic handling of encodings & documents
        result.capabilities.position_encoding = Some(negotiated_position_encoding.into_lsp());
        result.capabilities.text_document_sync = Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                open_close: Some(true),
                save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                    include_text: Some(true),
                })),
                ..Default::default()
            },
        ));

        // 5. Make sure that the state now also uses the negotiated encoding
        self.state
            .set_position_encoding(negotiated_position_encoding);

        // 6. Emit a useful message about the negotiation, if enabled
        #[cfg(feature = "tracing")]
        {
            let mut lines = Vec::new();

            // 6a. Client name & version
            if let Some(info) = &params.client_info {
                if let Some(version) = &info.version {
                    lines.push(format!("{} v{}", info.name, version));
                } else {
                    lines.push(info.name.clone());
                }
            }

            // 6b. Workspace folders
            let num_folders = params
                .workspace_folders
                .as_deref()
                .unwrap_or_default()
                .len();
            lines.push(format!(
                "{} workspace folder{}",
                num_folders,
                if num_folders == 1 { "" } else { "s" }
            ));

            // 6c. Position encoding
            lines.push(format!(
                "{} position encoding",
                negotiated_position_encoding.as_str().to_ascii_uppercase(),
            ));

            info!(
                "Client negotiation was successful\n- {}",
                lines.join("\n- ")
            );
        }

        Box::pin(async move { Ok(result) })
    }

    // Document notification callbacks & content updating

    fn initialized(&mut self, _params: InitializedParams) -> ControlFlow<Result<()>> {
        ControlFlow::Continue(())
    }

    fn did_change_configuration(
        &mut self,
        _params: DidChangeConfigurationParams,
    ) -> ControlFlow<Result<()>> {
        ControlFlow::Continue(())
    }

    fn did_open(&mut self, params: DidOpenTextDocumentParams) -> ControlFlow<Result<()>> {
        #[cfg(feature = "tracing")]
        debug!("did_open: {}", params.text_document.uri);
        self.state.handle_document_open::<T>(params)
    }

    #[allow(unused_variables)]
    fn did_close(&mut self, params: DidCloseTextDocumentParams) -> ControlFlow<Result<()>> {
        #[cfg(feature = "tracing")]
        debug!("did_close: {}", params.text_document.uri);
        ControlFlow::Continue(())
    }

    fn did_change(&mut self, params: DidChangeTextDocumentParams) -> ControlFlow<Result<()>> {
        self.state.handle_document_change::<T>(params)
    }

    fn did_save(&mut self, params: DidSaveTextDocumentParams) -> ControlFlow<Result<()>> {
        #[cfg(feature = "tracing")]
        debug!("did_save: {}", params.text_document.uri);
        self.state.handle_document_save::<T>(params)
    }

    // async-lsp method name => our method name @ request type definition

    implement_methods!(
        hover                   => hover                 @ crate::requests::Hover,
        completion              => completion            @ crate::requests::Completion,
        completion_item_resolve => completion_resolve    @ crate::requests::CompletionResolve,
        code_action             => code_action           @ crate::requests::CodeAction,
        code_action_resolve     => code_action_resolve   @ crate::requests::CodeActionResolve,
        document_link           => link                  @ crate::requests::DocumentLink,
        document_link_resolve   => link_resolve          @ crate::requests::DocumentLinkResolve,
        declaration             => declaration           @ crate::requests::Declaration,
        definition              => definition            @ crate::requests::Definition,
        references              => references            @ crate::requests::References,
        rename                  => rename                @ crate::requests::Rename,
        prepare_rename          => rename_prepare        @ crate::requests::RenamePrepare,
        formatting              => document_format       @ crate::requests::DocumentFormat,
        range_formatting        => document_range_format @ crate::requests::DocumentRangeFormat,
        document_diagnostic     => document_diagnostics  @ crate::requests::DocumentDiagnostics,
    );
}
