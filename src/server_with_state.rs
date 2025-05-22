use std::{ops::ControlFlow, sync::Arc};

use futures::future::BoxFuture;

#[cfg(feature = "tracing")]
use tracing::{debug, info};

use async_lsp::{
    ClientSocket, ErrorCode, LanguageServer, ResponseError, Result,
    lsp_types::{
        CodeAction, CodeActionOrCommand, CodeActionParams, CompletionItem, CompletionParams,
        CompletionResponse, CompletionTextEdit, DidChangeConfigurationParams,
        DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
        DidSaveTextDocumentParams, DocumentFormattingParams, DocumentLink, DocumentLinkParams,
        DocumentRangeFormattingParams, GotoDefinitionParams, GotoDefinitionResponse, Hover,
        HoverParams, InitializeParams, InitializeResult, InitializedParams, Location, Position,
        PrepareRenameResponse, ReferenceParams, RenameParams, SaveOptions,
        TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind,
        TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TextEdit, Url, WorkspaceEdit,
        request::{GotoDeclarationParams, GotoDeclarationResponse},
    },
};

use crate::{
    server::Document,
    server_state::ServerState,
    server_trait::Server,
    text_utils::{Encoding, position_to_encoding},
};

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
    (
    	$our_function_name:ident,
    	$real_function_name:ident,

    	$params_ty:ty,
    	$result_ty:ty,

    	$try_extract_url:expr,
    	$pre_method_callback:expr,
    	$post_method_callback:expr
    ) => {
        fn $real_function_name(
            &mut self,
            mut params: $params_ty,
        ) -> BoxFuture<'static, Result<$result_ty, Self::Error>> {
            let server = Arc::clone(&self.server);
            let state = self.state.clone();
            Box::pin(async move {
                // 1. Try to extract the URL from the params for document tracking
                let url: Option<Url> = $try_extract_url(&params);
                let mut ver: Option<i32> = None;

                // 2. If we got an URL, track the document version & call the "pre method" callback
                if let Some(url) = url.as_ref() {
                    if let Some(doc) = state.document(url) {
                        ver.replace(doc.version());
                        $pre_method_callback(&state, &doc, &mut params);
                    }
                }

                // 3. Call the user-defined language server function
                let mut result = server.$our_function_name(state.clone(), params).await?;

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
                        // 4b. Version is not stale, run the final "post method" callback
                        $post_method_callback(&state, &doc, &mut result);
                    }
                }

                Ok(result)
            })
        }
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
        mut params: HoverParams,
    ) -> BoxFuture<'static, Result<Option<Hover>, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move {
            let Some(doc) = state.document(&params.text_document_position_params.text_document.uri)
            else {
                return Ok(None);
            };

            modify_incoming_position(
                &state,
                &doc,
                &mut params.text_document_position_params.position,
            );

            let mut hover = server.hover(state.clone(), params).await?;

            // FUTURE: Document may have changed during call, throw error if so?

            if let Some(hover) = hover.as_mut() {
                if let Some(range) = hover.range.as_mut() {
                    modify_outgoing_position(&state, &doc, &mut range.start);
                    modify_outgoing_position(&state, &doc, &mut range.end);
                }
            }

            Ok(hover)
        })
    }

    fn completion(
        &mut self,
        mut params: CompletionParams,
    ) -> BoxFuture<'static, Result<Option<CompletionResponse>, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move {
            let Some(doc) = state.document(&params.text_document_position.text_document.uri) else {
                return Ok(None);
            };

            modify_incoming_position(&state, &doc, &mut params.text_document_position.position);

            let mut response = server.completion(state.clone(), params).await?;

            // FUTURE: Document may have changed during call, throw error if so?

            if let Some(response) = response.as_mut() {
                let items = match response {
                    CompletionResponse::Array(v) => v,
                    CompletionResponse::List(v) => v.items.as_mut(),
                };
                for item in items {
                    if let Some(edit) = item.text_edit.as_mut() {
                        match edit {
                            CompletionTextEdit::Edit(edit) => {
                                modify_outgoing_position(&state, &doc, &mut edit.range.start);
                                modify_outgoing_position(&state, &doc, &mut edit.range.end);
                            }
                            CompletionTextEdit::InsertAndReplace(edit) => {
                                modify_outgoing_position(&state, &doc, &mut edit.insert.start);
                                modify_outgoing_position(&state, &doc, &mut edit.insert.end);
                                modify_outgoing_position(&state, &doc, &mut edit.replace.start);
                                modify_outgoing_position(&state, &doc, &mut edit.replace.end);
                            }
                        }
                    }
                }
            }

            Ok(response)
        })
    }

    fn completion_item_resolve(
        &mut self,
        item: CompletionItem,
    ) -> BoxFuture<'static, Result<CompletionItem, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move {
            // TODO: Modify incoming positions for edits & diagnostics ...

            let item = server.completion_resolve(state, item).await?;

            // FUTURE: Document may have changed during call, throw error if so?

            // TODO: Modify outgoing positions for edits & diagnostics ...

            Ok(item)
        })
    }

    fn code_action(
        &mut self,
        mut params: CodeActionParams,
    ) -> BoxFuture<'static, Result<Option<Vec<CodeActionOrCommand>>, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move {
            let Some(doc) = state.document(&params.text_document.uri) else {
                return Ok(None);
            };

            modify_incoming_position(&state, &doc, &mut params.range.start);
            modify_incoming_position(&state, &doc, &mut params.range.end);

            let mut actions = server.code_action(state, params).await?;

            // FUTURE: Document may have changed during call, throw error if so?

            if let Some(_actions) = actions.as_mut() {
                // TODO: Modify outgoing positions for document edits ...
            }

            Ok(actions)
        })
    }

    fn code_action_resolve(
        &mut self,
        item: CodeAction,
    ) -> BoxFuture<'static, Result<CodeAction, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move {
            // TODO: Modify incoming positions for edits & diagnostics ...

            let action = server.code_action_resolve(state, item).await?;

            // FUTURE: Document may have changed during call, throw error if so?

            // TODO: Modify outgoing positions for edits & diagnostics ...

            Ok(action)
        })
    }

    fn document_link(
        &mut self,
        params: DocumentLinkParams,
    ) -> BoxFuture<'static, Result<Option<Vec<DocumentLink>>, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move {
            let mut link = server.link(state, params).await?;

            // FUTURE: Document may have changed during call, throw error if so?

            if let Some(_links) = link.as_mut() {
                // TODO: Modify outgoing positions for document links ...
            }

            Ok(link)
        })
    }

    fn document_link_resolve(
        &mut self,
        mut link: DocumentLink,
    ) -> BoxFuture<'static, Result<DocumentLink, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(async move {
            let Some(doc) = link.target.as_ref().and_then(|url| state.document(url)) else {
                return Ok(link);
            };

            modify_incoming_position(&state, &doc, &mut link.range.start);
            modify_incoming_position(&state, &doc, &mut link.range.end);

            let mut link = server.link_resolve(state.clone(), link).await?;

            // FUTURE: Document may have changed during call, throw error if so?

            modify_outgoing_position(&state, &doc, &mut link.range.start);
            modify_outgoing_position(&state, &doc, &mut link.range.end);

            Ok(link)
        })
    }

    // Forwarding for: Definition, Declaration, References, Rename

    implement_method!(
        definition,
        definition,
        GotoDefinitionParams,
        Option<GotoDefinitionResponse>,
        |params: &GotoDefinitionParams| Some(
            params
                .text_document_position_params
                .text_document
                .uri
                .clone()
        ),
        |state: &ServerState, doc: &Document, params: &mut GotoDefinitionParams| {
            modify_incoming_position(
                state,
                doc,
                &mut params.text_document_position_params.position,
            );
        },
        |state: &ServerState, doc: &Document, result: &mut Option<GotoDefinitionResponse>| {
            if let Some(response) = result.as_mut() {
                match response {
                    GotoDefinitionResponse::Scalar(loc) => {
                        modify_outgoing_position(state, doc, &mut loc.range.start);
                        modify_outgoing_position(state, doc, &mut loc.range.end);
                    }
                    GotoDefinitionResponse::Array(locations) => {
                        for loc in locations.iter_mut() {
                            modify_outgoing_position(state, doc, &mut loc.range.start);
                            modify_outgoing_position(state, doc, &mut loc.range.end);
                        }
                    }
                    GotoDefinitionResponse::Link(links) => {
                        for link in links.iter_mut() {
                            if let Some(origin_range) = link.origin_selection_range.as_mut() {
                                modify_outgoing_position(state, doc, &mut origin_range.start);
                                modify_outgoing_position(state, doc, &mut origin_range.end);
                            }

                            modify_outgoing_position(state, doc, &mut link.target_range.start);
                            modify_outgoing_position(state, doc, &mut link.target_range.end);

                            modify_outgoing_position(
                                state,
                                doc,
                                &mut link.target_selection_range.start,
                            );
                            modify_outgoing_position(
                                state,
                                doc,
                                &mut link.target_selection_range.end,
                            );
                        }
                    }
                }
            }
        }
    );

    fn declaration(
        &mut self,
        params: GotoDeclarationParams,
    ) -> BoxFuture<'static, Result<Option<GotoDeclarationResponse>, Self::Error>> {
        self.definition(params) // Uses the exact same types, no need to repeat
    }

    implement_method!(
        references,
        references,
        ReferenceParams,
        Option<Vec<Location>>,
        |params: &ReferenceParams| Some(params.text_document_position.text_document.uri.clone()),
        |state: &ServerState, doc: &Document, params: &mut ReferenceParams| {
            modify_incoming_position(state, doc, &mut params.text_document_position.position);
        },
        |state: &ServerState, doc: &Document, result: &mut Option<Vec<Location>>| {
            if let Some(locations) = result.as_mut() {
                for loc in locations.iter_mut() {
                    modify_outgoing_position(state, doc, &mut loc.range.start);
                    modify_outgoing_position(state, doc, &mut loc.range.end);
                }
            }
        }
    );

    implement_method!(
        rename,
        rename,
        RenameParams,
        Option<WorkspaceEdit>,
        |params: &RenameParams| Some(params.text_document_position.text_document.uri.clone()),
        |state: &ServerState, doc: &Document, params: &mut RenameParams| {
            modify_incoming_position(state, doc, &mut params.text_document_position.position);
        },
        |state: &ServerState, doc: &Document, result: &mut Option<WorkspaceEdit>| {
            if let Some(response) = result.as_mut() {
                if let Some(changes) = response.changes.as_mut() {
                    for edits in changes.values_mut() {
                        for edit in edits.iter_mut() {
                            modify_outgoing_position(state, doc, &mut edit.range.start);
                            modify_outgoing_position(state, doc, &mut edit.range.end);
                        }
                    }
                }
            }
        }
    );

    implement_method!(
        rename_prepare,
        prepare_rename,
        TextDocumentPositionParams,
        Option<PrepareRenameResponse>,
        |params: &TextDocumentPositionParams| Some(params.text_document.uri.clone()),
        |state: &ServerState, doc: &Document, params: &mut TextDocumentPositionParams| {
            modify_incoming_position(state, doc, &mut params.position);
        },
        |state: &ServerState, doc: &Document, result: &mut Option<PrepareRenameResponse>| {
            if let Some(response) = result.as_mut() {
                match response {
                    PrepareRenameResponse::Range(range)
                    | PrepareRenameResponse::RangeWithPlaceholder { range, .. } => {
                        modify_outgoing_position(state, doc, &mut range.start);
                        modify_outgoing_position(state, doc, &mut range.end);
                    }
                    PrepareRenameResponse::DefaultBehavior { .. } => {}
                }
            }
        }
    );

    // Forwarding for: Formatting

    implement_method!(
        document_format,
        formatting,
        DocumentFormattingParams,
        Option<Vec<TextEdit>>,
        |params: &DocumentFormattingParams| Some(params.text_document.uri.clone()),
        |_, _, _| {},
        |state: &ServerState, doc: &Document, result: &mut Option<Vec<TextEdit>>| {
            if let Some(edits) = result.as_mut() {
                for edit in edits.iter_mut() {
                    modify_outgoing_position(state, doc, &mut edit.range.start);
                    modify_outgoing_position(state, doc, &mut edit.range.end);
                }
            }
        }
    );

    implement_method!(
        document_range_format,
        range_formatting,
        DocumentRangeFormattingParams,
        Option<Vec<TextEdit>>,
        |params: &DocumentRangeFormattingParams| Some(params.text_document.uri.clone()),
        |state: &ServerState, doc: &Document, params: &mut DocumentRangeFormattingParams| {
            modify_incoming_position(state, doc, &mut params.range.start);
            modify_incoming_position(state, doc, &mut params.range.end);
        },
        |state: &ServerState, doc: &Document, result: &mut Option<Vec<TextEdit>>| {
            if let Some(edits) = result.as_mut() {
                for edit in edits.iter_mut() {
                    modify_outgoing_position(state, doc, &mut edit.range.start);
                    modify_outgoing_position(state, doc, &mut edit.range.end);
                }
            }
        }
    );
}

fn modify_incoming_position(state: &ServerState, document: &Document, position: &mut Position) {
    *position = position_to_encoding(
        &document.text,
        *position,
        state.get_position_encoding(),
        Encoding::UTF8,
    );
}

fn modify_outgoing_position(state: &ServerState, document: &Document, position: &mut Position) {
    *position = position_to_encoding(
        &document.text,
        *position,
        Encoding::UTF8,
        state.get_position_encoding(),
    );
}
