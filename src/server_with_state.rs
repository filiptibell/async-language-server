use std::{ops::ControlFlow, sync::Arc};

use async_lsp::{
    ClientSocket, ErrorCode, LanguageServer, ResponseError, Result,
    lsp_types::{
        DidChangeConfigurationParams, DidChangeTextDocumentParams, DidChangeWorkspaceFoldersParams,
        DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
        InitializeParams, InitializeResult, InitializedParams, SaveOptions,
        TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
        TextDocumentSyncSaveOptions, Url, WorkspaceDiagnosticParams,
        WorkspaceDiagnosticReportResult, WorkspaceFolder,
    },
};
use futures::future::BoxFuture;

#[cfg(feature = "tracing")]
use tracing::{debug, info};

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

fn workspace_folders(params: &InitializeParams) -> Vec<WorkspaceFolder> {
    if let Some(folders) = params.workspace_folders.clone() {
        return folders;
    }

    #[allow(deprecated)]
    params
        .root_uri
        .clone()
        .map(|uri| {
            let name = uri
                .to_file_path()
                .ok()
                .and_then(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().to_string())
                })
                .unwrap_or_else(|| "workspace".to_string());
            WorkspaceFolder { uri, name }
        })
        .into_iter()
        .collect()
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
        let workspace_folders = workspace_folders(&params);

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
        crate::workspace_diagnostics::enable_capabilities(&mut result);

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
        self.state.set_workspace_folders(workspace_folders.clone());

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
            let num_folders = workspace_folders.len();
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

    fn did_change_workspace_folders(
        &mut self,
        params: DidChangeWorkspaceFoldersParams,
    ) -> ControlFlow<Result<()>> {
        self.state.handle_workspace_folders_change(params)
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
        self.state.handle_document_close::<T>(params)
    }

    fn did_change(&mut self, params: DidChangeTextDocumentParams) -> ControlFlow<Result<()>> {
        self.state.handle_document_change::<T>(params)
    }

    fn did_save(&mut self, params: DidSaveTextDocumentParams) -> ControlFlow<Result<()>> {
        #[cfg(feature = "tracing")]
        debug!("did_save: {}", params.text_document.uri);
        self.state.handle_document_save::<T>(params)
    }

    fn workspace_diagnostic(
        &mut self,
        params: WorkspaceDiagnosticParams,
    ) -> BoxFuture<'static, Result<WorkspaceDiagnosticReportResult, Self::Error>> {
        let server = Arc::clone(&self.server);
        let state = self.state.clone();
        Box::pin(crate::workspace_diagnostics::workspace_diagnostic(
            server, state, params,
        ))
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

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use async_lsp::{
        ClientSocket, LanguageServer,
        lsp_types::{
            ClientCapabilities, Diagnostic, DiagnosticOptions, DiagnosticServerCapabilities,
            DidChangeWorkspaceFoldersParams, DidOpenTextDocumentParams, DocumentDiagnosticParams,
            DocumentDiagnosticReport, DocumentDiagnosticReportKind, DocumentDiagnosticReportResult,
            FullDocumentDiagnosticReport, InitializeParams, OneOf, PartialResultParams, Position,
            PreviousResultId, Range, RelatedFullDocumentDiagnosticReport, ServerCapabilities,
            TextDocumentItem, Url, WorkDoneProgressParams, WorkspaceDiagnosticParams,
            WorkspaceDiagnosticReportResult, WorkspaceDocumentDiagnosticReport, WorkspaceFolder,
            WorkspaceFoldersChangeEvent,
        },
    };

    use crate::{
        server::{DocumentMatcher, Server, ServerResult, ServerState},
        server_with_state::LanguageServerWithState,
    };

    struct TestServer;

    impl Server for TestServer {
        fn server_capabilities(_: ClientCapabilities) -> Option<ServerCapabilities> {
            Some(ServerCapabilities {
                diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
                    DiagnosticOptions {
                        inter_file_dependencies: true,
                        workspace_diagnostics: false,
                        ..Default::default()
                    },
                )),
                ..Default::default()
            })
        }

        fn server_document_matchers() -> Vec<DocumentMatcher> {
            vec![
                DocumentMatcher::new("Test")
                    .with_url_globs(["**/*.test", "*.test"])
                    .with_lang_strings(["test"]),
            ]
        }

        async fn document_diagnostics(
            &self,
            state: ServerState,
            params: DocumentDiagnosticParams,
        ) -> ServerResult<DocumentDiagnosticReportResult> {
            let message = if let Some(previous) = params.previous_result_id {
                format!("{}:{previous}", params.identifier.unwrap_or_default())
            } else {
                state
                    .document(&params.text_document.uri)
                    .map_or_else(String::new, |doc| doc.text_contents())
            };
            let related_documents = if message == "source" {
                related_uri(&params.text_document.uri).map(|uri| {
                    HashMap::from([(
                        uri,
                        DocumentDiagnosticReportKind::Full(FullDocumentDiagnosticReport {
                            result_id: None,
                            items: vec![diagnostic("related")],
                        }),
                    )])
                })
            } else {
                None
            };

            Ok(DocumentDiagnosticReportResult::Report(
                DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                    related_documents,
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: None,
                        items: vec![diagnostic(message)],
                    },
                }),
            ))
        }
    }

    #[test]
    fn initialize_enables_workspace_diagnostics() {
        let root = temp_workspace("capabilities");
        let mut server = LanguageServerWithState::new(ClientSocket::new_closed(), TestServer);

        let result = futures::executor::block_on(server.initialize(initialize_params(&root)))
            .expect("server can initialize");

        let Some(DiagnosticServerCapabilities::Options(options)) =
            result.capabilities.diagnostic_provider
        else {
            panic!("expected diagnostic options");
        };
        assert!(options.workspace_diagnostics);

        let Some(workspace) = result.capabilities.workspace else {
            panic!("expected workspace capabilities");
        };
        let Some(folders) = workspace.workspace_folders else {
            panic!("expected workspace folder capabilities");
        };
        assert_eq!(folders.supported, Some(true));
        assert_eq!(folders.change_notifications, Some(OneOf::Left(true)));

        fs::remove_dir_all(root).expect("temp workspace can be removed");
    }

    #[test]
    fn workspace_diagnostics_report_unopened_documents_without_versions() {
        let root = temp_workspace("workspace-diagnostics");
        let file = root.join("a.test");
        fs::write(&file, "disk").expect("test file can be written");

        let mut server = LanguageServerWithState::new(ClientSocket::new_closed(), TestServer);
        futures::executor::block_on(server.initialize(initialize_params(&root)))
            .expect("server can initialize");

        let report =
            futures::executor::block_on(server.workspace_diagnostic(workspace_diagnostic_params()))
                .expect("workspace diagnostics can be fetched");

        let WorkspaceDiagnosticReportResult::Report(report) = report else {
            panic!("expected full workspace diagnostic report");
        };
        let [WorkspaceDocumentDiagnosticReport::Full(report)] = report.items.as_slice() else {
            panic!("expected one full document report");
        };
        assert_eq!(report.version, None);
        assert_eq!(
            report.full_document_diagnostic_report.items[0].message,
            "disk"
        );

        fs::remove_dir_all(root).expect("temp workspace can be removed");
    }

    #[test]
    fn workspace_diagnostics_use_open_document_versions() {
        let root = temp_workspace("open-workspace-diagnostics");
        let file = root.join("a.test");
        fs::write(&file, "disk").expect("test file can be written");
        let file = fs::canonicalize(file).expect("test file can be canonicalized");
        let uri = Url::from_file_path(&file).expect("path can be converted to a URL");

        let mut server = LanguageServerWithState::new(ClientSocket::new_closed(), TestServer);
        futures::executor::block_on(server.initialize(initialize_params(&root)))
            .expect("server can initialize");
        let _ = server.did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem::new(uri, "test".into(), 3, "open".into()),
        });

        let report =
            futures::executor::block_on(server.workspace_diagnostic(workspace_diagnostic_params()))
                .expect("workspace diagnostics can be fetched");

        let WorkspaceDiagnosticReportResult::Report(report) = report else {
            panic!("expected full workspace diagnostic report");
        };
        let [WorkspaceDocumentDiagnosticReport::Full(report)] = report.items.as_slice() else {
            panic!("expected one full document report");
        };
        assert_eq!(report.version, Some(3));
        assert_eq!(
            report.full_document_diagnostic_report.items[0].message,
            "open"
        );

        fs::remove_dir_all(root).expect("temp workspace can be removed");
    }

    #[test]
    fn workspace_diagnostics_forward_previous_result_ids() {
        let root = temp_workspace("previous-result-id");
        let file = root.join("a.test");
        fs::write(&file, "disk").expect("test file can be written");
        let file = fs::canonicalize(file).expect("test file can be canonicalized");
        let uri = Url::from_file_path(file).expect("path can be converted to a URL");

        let mut server = LanguageServerWithState::new(ClientSocket::new_closed(), TestServer);
        futures::executor::block_on(server.initialize(initialize_params(&root)))
            .expect("server can initialize");

        let report =
            futures::executor::block_on(server.workspace_diagnostic(WorkspaceDiagnosticParams {
                identifier: Some("test".into()),
                previous_result_ids: vec![PreviousResultId {
                    uri,
                    value: "cached".into(),
                }],
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            }))
            .expect("workspace diagnostics can be fetched");

        let WorkspaceDiagnosticReportResult::Report(report) = report else {
            panic!("expected full workspace diagnostic report");
        };
        let [WorkspaceDocumentDiagnosticReport::Full(report)] = report.items.as_slice() else {
            panic!("expected one full document report");
        };
        assert_eq!(
            report.full_document_diagnostic_report.items[0].message,
            "test:cached"
        );

        fs::remove_dir_all(root).expect("temp workspace can be removed");
    }

    #[test]
    fn workspace_folder_changes_are_used_by_workspace_diagnostics() {
        let first = temp_workspace("workspace-folder-change-first");
        let second = temp_workspace("workspace-folder-change-second");
        fs::write(first.join("a.test"), "first").expect("test file can be written");
        fs::write(second.join("b.test"), "second").expect("test file can be written");

        let mut server = LanguageServerWithState::new(ClientSocket::new_closed(), TestServer);
        futures::executor::block_on(server.initialize(initialize_params(&first)))
            .expect("server can initialize");
        let _ = server.did_change_workspace_folders(DidChangeWorkspaceFoldersParams {
            event: WorkspaceFoldersChangeEvent {
                added: vec![workspace_folder(&second)],
                removed: vec![workspace_folder(&first)],
            },
        });

        let report =
            futures::executor::block_on(server.workspace_diagnostic(workspace_diagnostic_params()))
                .expect("workspace diagnostics can be fetched");

        let WorkspaceDiagnosticReportResult::Report(report) = report else {
            panic!("expected full workspace diagnostic report");
        };
        let [WorkspaceDocumentDiagnosticReport::Full(report)] = report.items.as_slice() else {
            panic!("expected one full document report");
        };
        assert_eq!(
            report.full_document_diagnostic_report.items[0].message,
            "second"
        );

        fs::remove_dir_all(first).expect("temp workspace can be removed");
        fs::remove_dir_all(second).expect("temp workspace can be removed");
    }

    #[test]
    fn workspace_diagnostics_prefer_direct_reports_over_related_reports() {
        let root = temp_workspace("related-reports");
        fs::write(root.join("a.test"), "source").expect("test file can be written");
        fs::write(root.join("b.test"), "direct").expect("test file can be written");

        let mut server = LanguageServerWithState::new(ClientSocket::new_closed(), TestServer);
        futures::executor::block_on(server.initialize(initialize_params(&root)))
            .expect("server can initialize");

        let report =
            futures::executor::block_on(server.workspace_diagnostic(workspace_diagnostic_params()))
                .expect("workspace diagnostics can be fetched");

        let WorkspaceDiagnosticReportResult::Report(report) = report else {
            panic!("expected full workspace diagnostic report");
        };
        let messages: Vec<_> = report.items.iter().map(workspace_report_message).collect();
        assert_eq!(messages, ["source", "direct"]);

        fs::remove_dir_all(root).expect("temp workspace can be removed");
    }

    fn temp_workspace(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after epoch")
            .as_millis();
        let root =
            std::env::temp_dir().join(format!("async-language-server-workspace-{name}-{millis}"));
        fs::create_dir_all(&root).expect("temp workspace can be created");
        root
    }

    fn initialize_params(root: &PathBuf) -> InitializeParams {
        InitializeParams {
            process_id: Some(std::process::id()),
            capabilities: ClientCapabilities::default(),
            workspace_folders: Some(vec![workspace_folder(root)]),
            ..Default::default()
        }
    }

    fn workspace_folder(path: &PathBuf) -> WorkspaceFolder {
        let uri = Url::from_file_path(path).expect("path can be converted to a URL");
        WorkspaceFolder {
            uri,
            name: "test".into(),
        }
    }

    fn workspace_diagnostic_params() -> WorkspaceDiagnosticParams {
        WorkspaceDiagnosticParams {
            identifier: None,
            previous_result_ids: Vec::new(),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        }
    }

    fn related_uri(uri: &Url) -> Option<Url> {
        let path = uri.to_file_path().ok()?.with_file_name("b.test");
        Url::from_file_path(path).ok()
    }

    fn workspace_report_message(report: &WorkspaceDocumentDiagnosticReport) -> &str {
        match report {
            WorkspaceDocumentDiagnosticReport::Full(report) => {
                report.full_document_diagnostic_report.items[0]
                    .message
                    .as_str()
            }
            WorkspaceDocumentDiagnosticReport::Unchanged(_) => panic!("expected full report"),
        }
    }

    fn diagnostic(message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            message: message.into(),
            ..Default::default()
        }
    }
}
