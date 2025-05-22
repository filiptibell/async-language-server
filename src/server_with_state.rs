use std::{ops::ControlFlow, sync::Arc};

use futures::future::BoxFuture;

#[cfg(feature = "tracing")]
use tracing::{debug, info};

use async_lsp::{
    ClientSocket, LanguageServer, ResponseError, Result,
    lsp_types::{
        CodeAction, CodeActionOrCommand, CodeActionParams, CompletionItem, CompletionParams,
        CompletionResponse, DidChangeConfigurationParams, DidChangeTextDocumentParams,
        DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
        DocumentFormattingParams, DocumentLink, DocumentLinkParams, DocumentRangeFormattingParams,
        GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams, InitializeParams,
        InitializeResult, InitializedParams, Location, PrepareRenameResponse, ReferenceParams,
        RenameParams, SaveOptions, TextDocumentPositionParams, TextDocumentSyncCapability,
        TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TextEdit,
        WorkspaceEdit,
        request::{GotoDeclarationParams, GotoDeclarationResponse},
    },
};

use crate::{server_state::ServerState, server_trait::Server, text_utils::PositionEncoding};

const POSITION_ENCODING_PREFERRED_ORDER: [PositionEncoding; 3] = [
    // First, prefer to use UTF-32 encoding, since this is
    // practically zero-cost for anything that Ropey needs
    PositionEncoding::UTF32,
    // Second, prefer to use UTF-8 encoding, since this is still
    // quite low cost to convert, depending on the text contents
    PositionEncoding::UTF8,
    // Lastly, use the standard UTF-16 encoding, which is universally
    // terrible, but also universally supported by all LSP clients
    PositionEncoding::UTF16,
];

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
        let mut negotiated_position_encoding = PositionEncoding::default();
        if let Some(client_available_encodings) = client_position_encodings {
            let client_available_encodings: Vec<PositionEncoding> = client_available_encodings
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
                    lines.push(info.name.to_string());
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

    // Forwarding for: Hover, Completion, Code Action, Document Link

    fn hover(
        &mut self,
        params: HoverParams,
    ) -> BoxFuture<'static, Result<Option<Hover>, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move { Ok(server.hover(state, params).await?) })
    }

    fn completion(
        &mut self,
        params: CompletionParams,
    ) -> BoxFuture<'static, Result<Option<CompletionResponse>, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move { Ok(server.completion(state, params).await?) })
    }

    fn completion_item_resolve(
        &mut self,
        item: CompletionItem,
    ) -> BoxFuture<'static, Result<CompletionItem, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move { Ok(server.completion_resolve(state, item).await?) })
    }

    fn code_action(
        &mut self,
        params: CodeActionParams,
    ) -> BoxFuture<'static, Result<Option<Vec<CodeActionOrCommand>>, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move { Ok(server.code_action(state, params).await?) })
    }

    fn code_action_resolve(
        &mut self,
        item: CodeAction,
    ) -> BoxFuture<'static, Result<CodeAction, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move { Ok(server.code_action_resolve(state, item).await?) })
    }

    fn document_link(
        &mut self,
        params: DocumentLinkParams,
    ) -> BoxFuture<'static, Result<Option<Vec<DocumentLink>>, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move { Ok(server.link(state, params).await?) })
    }

    fn document_link_resolve(
        &mut self,
        link: DocumentLink,
    ) -> BoxFuture<'static, Result<DocumentLink, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move { Ok(server.link_resolve(state, link).await?) })
    }

    // Forwarding for: Declaration, definition, References, Rename

    fn declaration(
        &mut self,
        params: GotoDeclarationParams,
    ) -> BoxFuture<'static, Result<Option<GotoDeclarationResponse>, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move { Ok(server.declaration(state, params).await?) })
    }

    fn definition(
        &mut self,
        params: GotoDefinitionParams,
    ) -> BoxFuture<'static, Result<Option<GotoDefinitionResponse>, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move { Ok(server.definition(state, params).await?) })
    }

    fn references(
        &mut self,
        params: ReferenceParams,
    ) -> BoxFuture<'static, Result<Option<Vec<Location>>, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move { Ok(server.references(state, params).await?) })
    }

    fn rename(
        &mut self,
        params: RenameParams,
    ) -> BoxFuture<'static, Result<Option<WorkspaceEdit>, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move { Ok(server.rename(state, params).await?) })
    }

    fn prepare_rename(
        &mut self,
        params: TextDocumentPositionParams,
    ) -> BoxFuture<'static, Result<Option<PrepareRenameResponse>, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move { Ok(server.rename_prepare(state, params).await?) })
    }

    // Forwarding for: Formatting

    fn formatting(
        &mut self,
        params: DocumentFormattingParams,
    ) -> BoxFuture<'static, Result<Option<Vec<TextEdit>>, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move { Ok(server.document_format(state, params).await?) })
    }

    fn range_formatting(
        &mut self,
        params: DocumentRangeFormattingParams,
    ) -> BoxFuture<'static, Result<Option<Vec<TextEdit>>, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move { Ok(server.document_range_format(state, params).await?) })
    }
}
