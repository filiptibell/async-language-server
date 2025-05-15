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
        DocumentLink, DocumentLinkParams, GotoDefinitionParams, GotoDefinitionResponse, Hover,
        HoverParams, InitializeParams, InitializeResult, InitializedParams, Location,
        PositionEncodingKind, PrepareRenameResponse, ReferenceParams, RenameParams, SaveOptions,
        TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind,
        TextDocumentSyncOptions, TextDocumentSyncSaveOptions, WorkspaceEdit,
        request::{GotoDeclarationParams, GotoDeclarationResponse},
    },
};

use crate::{server_state::ServerState, server_trait::Server};

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
        let mut result = InitializeResult {
            server_info: T::server_info(),
            capabilities: T::server_capabilities(params.capabilities).unwrap_or_default(),
        };

        result.capabilities.position_encoding = Some(PositionEncodingKind::UTF32);
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

        #[cfg(feature = "tracing")]
        {
            let num_folders = params
                .workspace_folders
                .as_deref()
                .unwrap_or_default()
                .len();

            if let Some(info) = &params.client_info {
                if let Some(version) = &info.version {
                    info!(
                        "Client connected - {} v{} - {} workspace folder{}",
                        info.name,
                        version,
                        num_folders,
                        if num_folders == 1 { "" } else { "s" }
                    );
                } else {
                    info!(
                        "Client connected - {} - {} workspace folder{}",
                        info.name,
                        num_folders,
                        if num_folders == 1 { "" } else { "s" }
                    );
                }
            } else {
                info!(
                    "Client connected - {} workspace folder{}",
                    num_folders,
                    if num_folders == 1 { "" } else { "s" }
                );
            }
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
}
