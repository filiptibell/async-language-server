use std::{collections::HashMap, sync::Arc};

use async_lsp::{
    ErrorCode, ResponseError, Result,
    lsp_types::{
        DiagnosticServerCapabilities, DocumentDiagnosticParams, DocumentDiagnosticReport,
        DocumentDiagnosticReportKind, DocumentDiagnosticReportResult, InitializeResult, OneOf,
        PartialResultParams, TextDocumentIdentifier, Url, WorkDoneProgressParams,
        WorkspaceDiagnosticParams, WorkspaceDiagnosticReport, WorkspaceDiagnosticReportResult,
        WorkspaceDocumentDiagnosticReport, WorkspaceFoldersServerCapabilities,
        WorkspaceFullDocumentDiagnosticReport, WorkspaceServerCapabilities,
        WorkspaceUnchangedDocumentDiagnosticReport,
    },
};

use crate::{requests::Request, server_state::ServerState, server_trait::Server};

pub(crate) fn enable_capabilities(result: &mut InitializeResult) {
    enable_workspace_diagnostics(result);
    enable_workspace_folder_tracking(result);
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

pub(crate) async fn workspace_diagnostic<T>(
    server: Arc<T>,
    state: ServerState,
    params: WorkspaceDiagnosticParams,
) -> Result<WorkspaceDiagnosticReportResult, ResponseError>
where
    T: Server + Send + Sync + 'static,
{
    let items = workspace_diagnostic_items(server, state, params).await?;
    Ok(WorkspaceDiagnosticReportResult::Report(
        WorkspaceDiagnosticReport { items },
    ))
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
