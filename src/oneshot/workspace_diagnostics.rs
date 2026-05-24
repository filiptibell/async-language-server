use std::{fs, path::PathBuf};

use async_lsp::lsp_types::{
    Diagnostic, DocumentDiagnosticReport, DocumentDiagnosticReportKind,
    DocumentDiagnosticReportResult, Url,
};

use crate::{
    document_matcher::DocumentMatchers,
    result::ServerResult,
    server_trait::Server,
    workspace_walker::{WorkspaceWalkConfig, WorkspaceWalker, path_to_url},
};

use super::server::{OneshotDocument, OneshotServer};

/**
    Configuration for running a language server once over a workspace.
*/
#[derive(Debug, Clone)]
pub struct WorkspaceDiagnosticConfig {
    roots: Vec<PathBuf>,
    walk: WorkspaceWalkConfig,
}

impl WorkspaceDiagnosticConfig {
    /**
        Creates a new workspace diagnostic configuration for the given root.
    */
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            roots: vec![root.into()],
            walk: WorkspaceWalkConfig::default(),
        }
    }

    /**
        Adds another root to scan for matching documents.
    */
    #[must_use]
    pub fn with_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.roots.push(root.into());
        self
    }

    /**
        Adds several more roots to scan for matching documents.
    */
    #[must_use]
    pub fn with_roots(mut self, root: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        self.roots.extend(root.into_iter().map(Into::into));
        self
    }

    /**
        Controls whether hidden files are included when scanning roots.
    */
    #[must_use]
    pub fn with_hidden_files(mut self, yes: bool) -> Self {
        self.walk = self.walk.with_hidden_files(yes);
        self
    }

    /**
        Controls whether `.gitignore` and related ignore files are respected.
    */
    #[must_use]
    pub fn with_ignore_files(mut self, yes: bool) -> Self {
        self.walk = self.walk.with_ignore_files(yes);
        self
    }
}

/**
    Diagnostics produced by running a server over a workspace.
*/
#[derive(Debug, Clone)]
pub struct WorkspaceDiagnosticReport {
    pub documents: Vec<DocumentDiagnostics>,
}

impl WorkspaceDiagnosticReport {
    /**
        Returns `true` if no document reported diagnostics.
    */
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.documents.iter().all(DocumentDiagnostics::is_empty)
    }
}

/**
    Diagnostics produced for a single document in a workspace.
*/
#[derive(Debug, Clone)]
pub struct DocumentDiagnostics {
    pub uri: Url,
    pub version: i32,
    pub report: DocumentDiagnosticReportResult,
}

impl DocumentDiagnostics {
    /**
        Returns `true` if this document diagnostic result contains no diagnostics.
    */
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics().is_empty()
    }

    /**
        Gets all diagnostics contained in this document diagnostic result.
    */
    #[must_use]
    pub fn diagnostics(&self) -> Vec<&Diagnostic> {
        match &self.report {
            DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
                report
                    .related_documents
                    .iter()
                    .flat_map(|related| related.values())
                    .flat_map(diagnostics_from_report_kind)
                    .chain(report.full_document_diagnostic_report.items.iter())
                    .collect()
            }
            DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Unchanged(report)) => {
                report
                    .related_documents
                    .iter()
                    .flat_map(|related| related.values())
                    .flat_map(diagnostics_from_report_kind)
                    .collect()
            }
            DocumentDiagnosticReportResult::Partial(report) => report
                .related_documents
                .iter()
                .flat_map(|related| related.values())
                .flat_map(diagnostics_from_report_kind)
                .collect(),
        }
    }
}

/**
    Runs workspace diagnostics against a server without starting an LSP transport.

    This uses the same stateful server wrapper as the regular transport path,
    but drives initialization, document opening, and diagnostic requests directly.

    # Errors

    Returns an error if a workspace root cannot be read, a matched document
    cannot be opened, or the server returns an error from a diagnostic request.
*/
pub async fn workspace_diagnostics<S>(
    server: S,
    config: WorkspaceDiagnosticConfig,
) -> ServerResult<WorkspaceDiagnosticReport>
where
    S: Server + Send + Sync + 'static,
{
    let walker = WorkspaceWalker::new(&config.roots, config.walk)?;
    let documents = discover_documents::<S>(&walker)?;

    let mut server = OneshotServer::new(server);
    server.initialize_workspace(walker.roots()).await?;
    for doc in &documents {
        server.open_document(&doc.document)?;
    }

    let mut results = Vec::new();
    for doc in documents {
        let report = server.document_diagnostics(&doc.document).await?;
        results.push(DocumentDiagnostics {
            uri: doc.document.uri,
            version: doc.document.version,
            report,
        });
    }

    Ok(WorkspaceDiagnosticReport { documents: results })
}

#[derive(Debug, Clone)]
struct WorkspaceDocument {
    path: PathBuf,
    document: OneshotDocument,
}

fn discover_documents<S>(walker: &WorkspaceWalker) -> ServerResult<Vec<WorkspaceDocument>>
where
    S: Server,
{
    let matchers = DocumentMatchers::new(S::server_document_matchers());
    let mut documents = Vec::new();

    for path in walker.files()? {
        if let Some(doc) = workspace_document(path, &matchers)? {
            documents.push(doc);
        }
    }

    documents.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(documents)
}

fn workspace_document(
    path: PathBuf,
    matchers: &DocumentMatchers,
) -> ServerResult<Option<WorkspaceDocument>> {
    let uri = path_to_url(&path)?;
    let Some(matcher) = matchers.find_url(&uri) else {
        return Ok(None);
    };

    let language_id = matcher
        .lang_strings
        .first()
        .cloned()
        .unwrap_or_else(|| matcher.name.to_ascii_lowercase());

    Ok(Some(WorkspaceDocument {
        document: OneshotDocument {
            uri,
            text: fs::read_to_string(&path)?,
            language_id,
            version: 1,
        },
        path,
    }))
}

fn diagnostics_from_report_kind(report: &DocumentDiagnosticReportKind) -> &[Diagnostic] {
    match report {
        DocumentDiagnosticReportKind::Full(report) => &report.items,
        DocumentDiagnosticReportKind::Unchanged(_) => &[],
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use async_lsp::lsp_types::{
        Diagnostic, DocumentDiagnosticParams, FullDocumentDiagnosticReport, Position, Range,
        RelatedFullDocumentDiagnosticReport,
    };

    use crate::server::{DocumentMatcher, Server, ServerResult, ServerState};

    use super::{WorkspaceDiagnosticConfig, workspace_diagnostics};

    struct TestServer;

    impl Server for TestServer {
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
            _params: DocumentDiagnosticParams,
        ) -> ServerResult<async_lsp::lsp_types::DocumentDiagnosticReportResult> {
            Ok(full_report(vec![diagnostic(format!(
                "{} documents",
                state.documents().len()
            ))]))
        }
    }

    #[test]
    fn workspace_diagnostics_discovers_matching_documents() {
        let root = temp_workspace("discovers");
        fs::write(root.join("a.test"), "").expect("test file can be written");
        fs::write(root.join("b.txt"), "").expect("ignored file can be written");
        fs::create_dir_all(root.join("nested")).expect("nested dir can be created");
        fs::write(root.join("nested").join("c.test"), "").expect("nested file can be written");

        let report = futures::executor::block_on(workspace_diagnostics(
            TestServer,
            WorkspaceDiagnosticConfig::new(&root),
        ))
        .expect("workspace diagnostics succeeds");

        assert_eq!(report.documents.len(), 2);
        assert!(
            report
                .documents
                .iter()
                .any(|doc| doc.uri.path().ends_with("/a.test"))
        );
        assert!(
            report
                .documents
                .iter()
                .any(|doc| doc.uri.path().ends_with("/nested/c.test"))
        );

        fs::remove_dir_all(root).expect("temp workspace can be removed");
    }

    #[test]
    fn workspace_diagnostics_respects_gitignore_by_default() {
        let root = temp_workspace("gitignore");
        fs::create_dir_all(root.join(".git")).expect("git dir can be created");
        fs::write(root.join(".gitignore"), "ignored/\n").expect("gitignore can be written");
        fs::write(root.join("a.test"), "").expect("test file can be written");
        fs::create_dir_all(root.join("ignored")).expect("ignored dir can be created");
        fs::write(root.join("ignored").join("b.test"), "").expect("ignored file can be written");

        let report = futures::executor::block_on(workspace_diagnostics(
            TestServer,
            WorkspaceDiagnosticConfig::new(&root),
        ))
        .expect("workspace diagnostics succeeds");

        assert_eq!(report.documents.len(), 1);
        assert!(report.documents[0].uri.path().ends_with("/a.test"));

        fs::remove_dir_all(root).expect("temp workspace can be removed");
    }

    #[test]
    fn workspace_diagnostics_can_disable_ignore_files() {
        let root = temp_workspace("ignore-disabled");
        fs::create_dir_all(root.join(".git")).expect("git dir can be created");
        fs::write(root.join(".gitignore"), "ignored/\n").expect("gitignore can be written");
        fs::write(root.join("a.test"), "").expect("test file can be written");
        fs::create_dir_all(root.join("ignored")).expect("ignored dir can be created");
        fs::write(root.join("ignored").join("b.test"), "").expect("ignored file can be written");

        let report = futures::executor::block_on(workspace_diagnostics(
            TestServer,
            WorkspaceDiagnosticConfig::new(&root).with_ignore_files(false),
        ))
        .expect("workspace diagnostics succeeds");

        assert_eq!(report.documents.len(), 2);
        assert!(
            report
                .documents
                .iter()
                .any(|doc| doc.uri.path().ends_with("/ignored/b.test"))
        );

        fs::remove_dir_all(root).expect("temp workspace can be removed");
    }

    #[test]
    fn workspace_diagnostics_opens_documents_before_requests() {
        let root = temp_workspace("opened");
        fs::write(root.join("a.test"), "").expect("test file can be written");
        fs::write(root.join("b.test"), "").expect("test file can be written");

        let report = futures::executor::block_on(workspace_diagnostics(
            TestServer,
            WorkspaceDiagnosticConfig::new(&root),
        ))
        .expect("workspace diagnostics succeeds");

        assert_eq!(report.documents.len(), 2);
        for doc in &report.documents {
            let diagnostics = doc.diagnostics();
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(diagnostics[0].message, "2 documents");
        }

        fs::remove_dir_all(root).expect("temp workspace can be removed");
    }

    fn temp_workspace(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after epoch")
            .as_millis();
        let root =
            std::env::temp_dir().join(format!("async-language-server-oneshot-{name}-{millis}"));
        fs::create_dir_all(&root).expect("temp workspace can be created");
        root
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

    fn full_report(items: Vec<Diagnostic>) -> async_lsp::lsp_types::DocumentDiagnosticReportResult {
        async_lsp::lsp_types::DocumentDiagnosticReportResult::Report(
            async_lsp::lsp_types::DocumentDiagnosticReport::Full(
                RelatedFullDocumentDiagnosticReport {
                    related_documents: None,
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: None,
                        items,
                    },
                },
            ),
        )
    }
}
