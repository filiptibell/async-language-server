use std::{
    collections::HashMap,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use async_lsp::{
    ErrorCode, ResponseError, Result,
    lsp_types::{
        ClientCapabilities, ConfigurationParams, DiagnosticServerCapabilities,
        DocumentDiagnosticParams, DocumentDiagnosticReport, DocumentDiagnosticReportKind,
        DocumentDiagnosticReportResult, FullDocumentDiagnosticReport, InitializeResult, LSPAny,
        OneOf, PartialResultParams, Registration, RegistrationParams, TextDocumentIdentifier, Url,
        WorkDoneProgressParams, WorkspaceDiagnosticParams, WorkspaceDiagnosticReport,
        WorkspaceDiagnosticReportResult, WorkspaceDocumentDiagnosticReport,
        WorkspaceFoldersServerCapabilities, WorkspaceFullDocumentDiagnosticReport,
        WorkspaceServerCapabilities, WorkspaceUnchangedDocumentDiagnosticReport,
        request::{RegisterCapability, WorkspaceConfiguration, WorkspaceDiagnosticRefresh},
    },
};

use crate::{
    requests::Request,
    server_options::{ServerOptions, WorkspaceDiagnostics, WorkspaceDiagnosticsSetting},
    server_state::ServerState,
    server_trait::Server,
};

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceDiagnosticsState {
    inner: Arc<WorkspaceDiagnosticsStateInner>,
}

#[derive(Debug)]
struct WorkspaceDiagnosticsStateInner {
    options: WorkspaceDiagnostics,
    supported: AtomicBool,
    enabled: AtomicBool,
    client_configuration: AtomicBool,
    client_dynamic_configuration: AtomicBool,
    client_refresh: AtomicBool,
    generation: AtomicU64,
}

impl WorkspaceDiagnosticsState {
    pub(crate) fn new(options: &ServerOptions) -> Self {
        let enabled = match &options.workspace_diagnostics {
            WorkspaceDiagnostics::Disabled => false,
            WorkspaceDiagnostics::Enabled => true,
            WorkspaceDiagnostics::Configurable(setting) => setting.default_enabled,
        };

        Self {
            inner: Arc::new(WorkspaceDiagnosticsStateInner {
                options: options.workspace_diagnostics.clone(),
                supported: AtomicBool::new(!matches!(
                    &options.workspace_diagnostics,
                    WorkspaceDiagnostics::Disabled
                )),
                enabled: AtomicBool::new(enabled),
                client_configuration: AtomicBool::new(false),
                client_dynamic_configuration: AtomicBool::new(false),
                client_refresh: AtomicBool::new(false),
                generation: AtomicU64::new(0),
            }),
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.supported() && self.inner.enabled.load(Ordering::Relaxed)
    }

    pub(crate) fn supported(&self) -> bool {
        self.inner.supported.load(Ordering::Relaxed)
    }

    fn setting(&self) -> Option<&WorkspaceDiagnosticsSetting> {
        if let WorkspaceDiagnostics::Configurable(setting) = &self.inner.options {
            Some(setting)
        } else {
            None
        }
    }

    fn can_request_configuration(&self) -> bool {
        self.inner.client_configuration.load(Ordering::Relaxed) && self.setting().is_some()
    }

    fn can_register_configuration(&self) -> bool {
        self.inner
            .client_dynamic_configuration
            .load(Ordering::Relaxed)
            && self.setting().is_some()
    }

    fn can_refresh(&self) -> bool {
        self.inner.client_refresh.load(Ordering::Relaxed)
    }

    fn configure(&self, result: &InitializeResult, client_capabilities: &ClientCapabilities) {
        self.inner.supported.store(
            !matches!(&self.inner.options, WorkspaceDiagnostics::Disabled)
                && result.capabilities.diagnostic_provider.is_some(),
            Ordering::Relaxed,
        );

        let workspace = client_capabilities.workspace.as_ref();
        self.inner.client_configuration.store(
            workspace.and_then(|w| w.configuration).unwrap_or(false),
            Ordering::Relaxed,
        );
        self.inner.client_dynamic_configuration.store(
            workspace
                .and_then(|w| w.did_change_configuration.as_ref())
                .and_then(|c| c.dynamic_registration)
                .unwrap_or(false),
            Ordering::Relaxed,
        );
        self.inner.client_refresh.store(
            workspace
                .and_then(|w| w.diagnostic.as_ref())
                .and_then(|d| d.refresh_support)
                .unwrap_or(false),
            Ordering::Relaxed,
        );
    }

    fn next_generation(&self) -> u64 {
        self.inner.generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn current_generation(&self) -> u64 {
        self.inner.generation.load(Ordering::Relaxed)
    }

    pub(crate) fn set_enabled(&self, enabled: bool) -> bool {
        self.inner.enabled.swap(enabled, Ordering::Relaxed) != enabled
    }
}

pub(crate) fn configure_capabilities(
    state: &ServerState,
    result: &mut InitializeResult,
    client_capabilities: &ClientCapabilities,
) {
    let workspace_diagnostics = state.workspace_diagnostics();
    workspace_diagnostics.configure(result, client_capabilities);

    match &workspace_diagnostics.inner.options {
        WorkspaceDiagnostics::Disabled => disable_workspace_diagnostics(result),
        WorkspaceDiagnostics::Enabled | WorkspaceDiagnostics::Configurable(_) => {
            enable_workspace_diagnostics(result);
            enable_workspace_folder_tracking(result);
        }
    }
}

fn enable_workspace_diagnostics(result: &mut InitializeResult) {
    if let Some(provider) = result.capabilities.diagnostic_provider.as_mut() {
        match provider {
            DiagnosticServerCapabilities::Options(options) => {
                options.workspace_diagnostics = true;
            }
            DiagnosticServerCapabilities::RegistrationOptions(options) => {
                options.diagnostic_options.workspace_diagnostics = true;
            }
        }
    }
}

fn disable_workspace_diagnostics(result: &mut InitializeResult) {
    if let Some(provider) = result.capabilities.diagnostic_provider.as_mut() {
        match provider {
            DiagnosticServerCapabilities::Options(options) => {
                options.workspace_diagnostics = false;
            }
            DiagnosticServerCapabilities::RegistrationOptions(options) => {
                options.diagnostic_options.workspace_diagnostics = false;
            }
        }
    }
}

fn enable_workspace_folder_tracking(result: &mut InitializeResult) {
    if result.capabilities.diagnostic_provider.is_none() {
        return;
    }

    let workspace = result
        .capabilities
        .workspace
        .get_or_insert_with(WorkspaceServerCapabilities::default);
    let folders = workspace
        .workspace_folders
        .get_or_insert_with(WorkspaceFoldersServerCapabilities::default);

    folders.supported = Some(true);
    if !matches!(folders.change_notifications, Some(OneOf::Right(_))) {
        folders.change_notifications = Some(OneOf::Left(true));
    }
}

pub(crate) fn apply_initialization_options(state: &ServerState, options: Option<&LSPAny>) {
    let Some(options) = options else {
        return;
    };
    let workspace_diagnostics = state.workspace_diagnostics();
    let Some(setting) = workspace_diagnostics.setting() else {
        return;
    };
    let Some(enabled) = setting.key.value(options) else {
        return;
    };

    state.set_workspace_diagnostics_enabled(enabled);
}

pub(crate) fn initialized(state: ServerState) {
    register_configuration(state.clone());
    request_configuration(state);
}

pub(crate) fn did_change_configuration(state: ServerState, settings: &LSPAny) {
    let workspace_diagnostics = state.workspace_diagnostics();
    let Some(setting) = workspace_diagnostics.setting() else {
        return;
    };

    if let Some(enabled) = setting.key.value(settings) {
        workspace_diagnostics.next_generation();
        apply_enabled(state, enabled);
    } else {
        request_configuration(state);
    }
}

pub(crate) async fn workspace_diagnostic<T>(
    server: Arc<T>,
    state: ServerState,
    params: WorkspaceDiagnosticParams,
) -> Result<WorkspaceDiagnosticReportResult, ResponseError>
where
    T: Server + Send + Sync + 'static,
{
    if !state.workspace_diagnostics().supported() {
        return Err(ResponseError::new(
            ErrorCode::METHOD_NOT_FOUND,
            "workspace diagnostics are disabled",
        ));
    }

    if !state.workspace_diagnostics().enabled() {
        return Ok(WorkspaceDiagnosticReportResult::Report(
            disabled_workspace_diagnostic_report(&state, params),
        ));
    }

    let items = workspace_diagnostic_items(server, state, params).await?;
    Ok(WorkspaceDiagnosticReportResult::Report(
        WorkspaceDiagnosticReport { items },
    ))
}

fn register_configuration(state: ServerState) {
    let workspace_diagnostics = state.workspace_diagnostics();
    if !workspace_diagnostics.can_register_configuration() {
        return;
    }
    let Some(setting) = workspace_diagnostics.setting().cloned() else {
        return;
    };

    spawn(async move {
        let _ = state
            .client()
            .request::<RegisterCapability>(RegistrationParams {
                registrations: vec![Registration {
                    id: "async-language-server.workspaceDiagnostics.configuration".into(),
                    method: "workspace/didChangeConfiguration".into(),
                    register_options: Some(serde_json::json!({
                        "section": setting.key.section(),
                    })),
                }],
            })
            .await;
    });
}

fn request_configuration(state: ServerState) {
    let workspace_diagnostics = state.workspace_diagnostics();
    if !workspace_diagnostics.can_request_configuration() {
        return;
    }
    let Some(setting) = workspace_diagnostics.setting().cloned() else {
        return;
    };
    let generation = workspace_diagnostics.next_generation();

    spawn(async move {
        let response = state
            .client()
            .request::<WorkspaceConfiguration>(ConfigurationParams {
                items: vec![setting.key.item()],
            })
            .await;
        let Ok(response) = response else {
            return;
        };
        if workspace_diagnostics.current_generation() != generation {
            return;
        }
        let Some(value) = response.first() else {
            return;
        };
        let Some(enabled) = setting.key.value(value) else {
            return;
        };

        apply_enabled(state, enabled);
    });
}

fn apply_enabled(state: ServerState, enabled: bool) {
    let refresh = state.set_workspace_diagnostics_enabled(enabled);
    if refresh && state.workspace_diagnostics().supported() {
        refresh_diagnostics(state);
    }
}

fn refresh_diagnostics(state: ServerState) {
    if !state.workspace_diagnostics().can_refresh() {
        return;
    }

    spawn(async move {
        let _ = state
            .client()
            .request::<WorkspaceDiagnosticRefresh>(())
            .await;
    });
}

fn spawn(future: impl Future<Output = ()> + Send + 'static) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(future);
    }
}

fn disabled_workspace_diagnostic_report(
    state: &ServerState,
    params: WorkspaceDiagnosticParams,
) -> WorkspaceDiagnosticReport {
    let items = params
        .previous_result_ids
        .into_iter()
        .map(|previous| {
            WorkspaceDocumentDiagnosticReport::Full(WorkspaceFullDocumentDiagnosticReport {
                version: state.document_workspace_version(&previous.uri),
                uri: previous.uri,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: None,
                    items: Vec::new(),
                },
            })
        })
        .collect();

    WorkspaceDiagnosticReport { items }
}

async fn workspace_diagnostic_items<T>(
    server: Arc<T>,
    state: ServerState,
    params: WorkspaceDiagnosticParams,
) -> Result<Vec<WorkspaceDocumentDiagnosticReport>, ResponseError>
where
    T: Server + Send + Sync + 'static,
{
    let identifier = params.identifier;
    let previous_result_ids: HashMap<_, _> = params
        .previous_result_ids
        .into_iter()
        .map(|id| (id.uri, id.value))
        .collect();
    let urls = state
        .refresh_workspace_documents::<T>()
        .map_err(ResponseError::from)?;
    let mut items = Vec::new();

    for url in urls {
        let Some(doc) = state.document(&url) else {
            continue;
        };
        let version = doc.version();
        let mut result = server
            .document_diagnostics(
                state.clone(),
                document_diagnostic_params(
                    url.clone(),
                    identifier.clone(),
                    previous_result_ids.get(&url).cloned(),
                ),
            )
            .await
            .map_err(ResponseError::from)?;

        if state
            .document(&url)
            .is_some_and(|doc| doc.version() != version)
        {
            return Err(ResponseError::new(
                ErrorCode::CONTENT_MODIFIED,
                "document was modified during processing",
            ));
        }

        <crate::requests::DocumentDiagnostics as Request>::modify_response(
            &state,
            &doc,
            &mut result,
        );
        push_workspace_reports_from_document_result(&state, url, result, &mut items);
    }

    items.sort_by(|a, b| {
        workspace_report_uri(a)
            .as_str()
            .cmp(workspace_report_uri(b).as_str())
    });
    Ok(items)
}

fn document_diagnostic_params(
    uri: Url,
    identifier: Option<String>,
    previous_result_id: Option<String>,
) -> DocumentDiagnosticParams {
    DocumentDiagnosticParams {
        text_document: TextDocumentIdentifier::new(uri),
        identifier,
        previous_result_id,
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn push_workspace_reports_from_document_result(
    state: &ServerState,
    uri: Url,
    result: DocumentDiagnosticReportResult,
    reports: &mut Vec<WorkspaceDocumentDiagnosticReport>,
) {
    match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) => {
            push_workspace_report(
                reports,
                WorkspaceDocumentDiagnosticReport::Full(WorkspaceFullDocumentDiagnosticReport {
                    version: state.document_workspace_version(&uri),
                    uri,
                    full_document_diagnostic_report: report.full_document_diagnostic_report,
                }),
                true,
            );
            push_related_reports(state, report.related_documents, reports);
        }
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Unchanged(report)) => {
            push_workspace_report(
                reports,
                WorkspaceDocumentDiagnosticReport::Unchanged(
                    WorkspaceUnchangedDocumentDiagnosticReport {
                        version: state.document_workspace_version(&uri),
                        uri,
                        unchanged_document_diagnostic_report: report
                            .unchanged_document_diagnostic_report,
                    },
                ),
                true,
            );
            push_related_reports(state, report.related_documents, reports);
        }
        DocumentDiagnosticReportResult::Partial(report) => {
            push_related_reports(state, report.related_documents, reports);
        }
    }
}

fn push_workspace_report(
    reports: &mut Vec<WorkspaceDocumentDiagnosticReport>,
    report: WorkspaceDocumentDiagnosticReport,
    replace: bool,
) {
    if let Some(index) = reports
        .iter()
        .position(|existing| workspace_report_uri(existing) == workspace_report_uri(&report))
    {
        if replace {
            reports[index] = report;
        }
    } else {
        reports.push(report);
    }
}

fn workspace_report_uri(report: &WorkspaceDocumentDiagnosticReport) -> &Url {
    match report {
        WorkspaceDocumentDiagnosticReport::Full(report) => &report.uri,
        WorkspaceDocumentDiagnosticReport::Unchanged(report) => &report.uri,
    }
}

fn push_related_reports(
    state: &ServerState,
    related_documents: Option<HashMap<Url, DocumentDiagnosticReportKind>>,
    reports: &mut Vec<WorkspaceDocumentDiagnosticReport>,
) {
    let Some(related_documents) = related_documents else {
        return;
    };

    for (uri, report) in related_documents {
        match report {
            DocumentDiagnosticReportKind::Full(report) => {
                push_workspace_report(
                    reports,
                    WorkspaceDocumentDiagnosticReport::Full(
                        WorkspaceFullDocumentDiagnosticReport {
                            version: state.document_workspace_version(&uri),
                            uri,
                            full_document_diagnostic_report: report,
                        },
                    ),
                    false,
                );
            }
            DocumentDiagnosticReportKind::Unchanged(report) => {
                push_workspace_report(
                    reports,
                    WorkspaceDocumentDiagnosticReport::Unchanged(
                        WorkspaceUnchangedDocumentDiagnosticReport {
                            version: state.document_workspace_version(&uri),
                            uri,
                            unchanged_document_diagnostic_report: report,
                        },
                    ),
                    false,
                );
            }
        }
    }
}
