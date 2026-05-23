use async_lsp::lsp_types::{
    CodeAction as LspCodeAction, CodeActionOrCommand as LspCodeActionOrCommand,
    CodeActionParams as LspCodeActionParams, CompletionItem as LspCompletionItem,
    CompletionParams as LspCompletionParams, CompletionResponse as LspCompletionResponse,
    CompletionTextEdit as LspCompletionTextEdit, Diagnostic as LspDiagnostic,
    DocumentDiagnosticParams, DocumentDiagnosticReport, DocumentDiagnosticReportResult,
    DocumentFormattingParams as LspDocumentFormattingParams, DocumentLink as LspDocumentLink,
    DocumentLinkParams as LspDocumentLinkParams,
    DocumentRangeFormattingParams as LspDocumentRangeFormattingParams,
    GotoDefinitionParams as LspGotoDefinitionParams,
    GotoDefinitionResponse as LspGotoDefinitionResponse, Hover as LspHover,
    HoverParams as LspHoverParams, Location as LspLocation, LocationLink as LspLocationLink, OneOf,
    Position as LspPosition, PrepareRenameResponse as LspPrepareRenameResponse, Range as LspRange,
    ReferenceParams as LspReferenceParams, RenameParams as LspRenameParams,
    TextDocumentPositionParams as LspTextDocumentPositionParams, TextEdit as LspTextEdit, Url,
    WorkspaceEdit as LspWorkspaceEdit,
    request::{
        GotoDeclarationParams as LspGotoDeclarationParams,
        GotoDeclarationResponse as LspGotoDeclarationResponse,
    },
};

use crate::{
    server::{Document, ServerState},
    text_utils::{Encoding, position_to_encoding},
};

// ════════════════════════════════
// Request Trait & Helper Functions
// ════════════════════════════════

#[allow(dead_code)]
#[allow(unused_variables)]
pub trait Request {
    type Params;
    type Response;

    fn extract_url(params: &Self::Params) -> Option<Url> {
        None
    }

    fn modify_params(state: &ServerState, document: &Document, params: &mut Self::Params) {}
    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {}
}

fn modify_incoming_position(state: &ServerState, document: &Document, position: &mut LspPosition) {
    *position = position_to_encoding(
        &document.text,
        *position,
        state.get_position_encoding(),
        Encoding::UTF8,
    );
}

fn modify_incoming_position_at_url(
    state: &ServerState,
    fallback: &Document,
    url: &Url,
    position: &mut LspPosition,
) {
    if url == fallback.url() {
        modify_incoming_position(state, fallback, position);
    } else if let Some(document) = state.document(url) {
        modify_incoming_position(state, &document, position);
    } else {
        modify_incoming_position(state, fallback, position);
    }
}

fn modify_incoming_range(state: &ServerState, document: &Document, range: &mut LspRange) {
    modify_incoming_position(state, document, &mut range.start);
    modify_incoming_position(state, document, &mut range.end);
}

fn modify_incoming_range_at_url(
    state: &ServerState,
    fallback: &Document,
    url: &Url,
    range: &mut LspRange,
) {
    modify_incoming_position_at_url(state, fallback, url, &mut range.start);
    modify_incoming_position_at_url(state, fallback, url, &mut range.end);
}

fn modify_incoming_location(state: &ServerState, document: &Document, loc: &mut LspLocation) {
    let uri = loc.uri.clone();
    modify_incoming_range_at_url(state, document, &uri, &mut loc.range);
}

fn modify_outgoing_position(state: &ServerState, document: &Document, position: &mut LspPosition) {
    *position = position_to_encoding(
        &document.text,
        *position,
        Encoding::UTF8,
        state.get_position_encoding(),
    );
}

fn modify_outgoing_position_at_url(
    state: &ServerState,
    fallback: &Document,
    url: &Url,
    position: &mut LspPosition,
) {
    if url == fallback.url() {
        modify_outgoing_position(state, fallback, position);
    } else if let Some(document) = state.document(url) {
        modify_outgoing_position(state, &document, position);
    } else {
        modify_outgoing_position(state, fallback, position);
    }
}

fn modify_outgoing_range(state: &ServerState, document: &Document, range: &mut LspRange) {
    modify_outgoing_position(state, document, &mut range.start);
    modify_outgoing_position(state, document, &mut range.end);
}

fn modify_outgoing_range_at_url(
    state: &ServerState,
    fallback: &Document,
    url: &Url,
    range: &mut LspRange,
) {
    modify_outgoing_position_at_url(state, fallback, url, &mut range.start);
    modify_outgoing_position_at_url(state, fallback, url, &mut range.end);
}

fn modify_outgoing_location(state: &ServerState, document: &Document, loc: &mut LspLocation) {
    let uri = loc.uri.clone();
    modify_outgoing_range_at_url(state, document, &uri, &mut loc.range);
}

fn modify_outgoing_text_edit(state: &ServerState, document: &Document, edit: &mut LspTextEdit) {
    modify_outgoing_range(state, document, &mut edit.range);
}

fn modify_incoming_diagnostic(state: &ServerState, document: &Document, diag: &mut LspDiagnostic) {
    modify_incoming_range(state, document, &mut diag.range);
    if let Some(related) = diag.related_information.as_mut() {
        for info in related {
            modify_incoming_location(state, document, &mut info.location);
        }
    }
}

fn modify_outgoing_diagnostic(state: &ServerState, document: &Document, diag: &mut LspDiagnostic) {
    modify_outgoing_range(state, document, &mut diag.range);
    if let Some(related) = diag.related_information.as_mut() {
        for info in related {
            modify_outgoing_location(state, document, &mut info.location);
        }
    }
}

fn modify_outgoing_completion_text_edit(
    state: &ServerState,
    document: &Document,
    edit: &mut LspCompletionTextEdit,
) {
    match edit {
        LspCompletionTextEdit::Edit(edit) => modify_outgoing_text_edit(state, document, edit),
        LspCompletionTextEdit::InsertAndReplace(edit) => {
            modify_outgoing_range(state, document, &mut edit.insert);
            modify_outgoing_range(state, document, &mut edit.replace);
        }
    }
}

fn modify_outgoing_location_link(
    state: &ServerState,
    document: &Document,
    link: &mut LspLocationLink,
) {
    if let Some(origin_range) = link.origin_selection_range.as_mut() {
        modify_outgoing_range(state, document, origin_range);
    }

    modify_outgoing_range_at_url(state, document, &link.target_uri, &mut link.target_range);
    modify_outgoing_range_at_url(
        state,
        document,
        &link.target_uri,
        &mut link.target_selection_range,
    );
}

fn modify_outgoing_workspace_edit(
    state: &ServerState,
    document: &Document,
    edit: &mut LspWorkspaceEdit,
) {
    use async_lsp::lsp_types::{DocumentChangeOperation, DocumentChanges};

    if let Some(changes) = edit.changes.as_mut() {
        for (uri, edits) in changes {
            for text_edit in edits.iter_mut() {
                modify_outgoing_range_at_url(state, document, uri, &mut text_edit.range);
            }
        }
    }

    if let Some(document_changes) = edit.document_changes.as_mut() {
        match document_changes {
            DocumentChanges::Edits(edits) => {
                for versioned_edit in edits.iter_mut() {
                    let uri = &versioned_edit.text_document.uri;
                    for text_edit in &mut versioned_edit.edits {
                        match text_edit {
                            OneOf::Left(l) => {
                                modify_outgoing_range_at_url(state, document, uri, &mut l.range);
                            }
                            OneOf::Right(r) => {
                                modify_outgoing_range_at_url(
                                    state,
                                    document,
                                    uri,
                                    &mut r.text_edit.range,
                                );
                            }
                        }
                    }
                }
            }
            DocumentChanges::Operations(ops) => {
                for op in ops.iter_mut() {
                    match op {
                        DocumentChangeOperation::Edit(edit) => {
                            let uri = &edit.text_document.uri;
                            for text_edit in &mut edit.edits {
                                match text_edit {
                                    OneOf::Left(l) => {
                                        modify_outgoing_range_at_url(
                                            state,
                                            document,
                                            uri,
                                            &mut l.range,
                                        );
                                    }
                                    OneOf::Right(r) => {
                                        modify_outgoing_range_at_url(
                                            state,
                                            document,
                                            uri,
                                            &mut r.text_edit.range,
                                        );
                                    }
                                }
                            }
                        }
                        DocumentChangeOperation::Op(_) => {
                            // File operations don't have positions to modify
                        }
                    }
                }
            }
        }
    }
}

// ═══════════════════════════
// Hover & Completion Requests
// ═══════════════════════════

pub struct Hover;

impl Request for Hover {
    type Params = LspHoverParams;
    type Response = Option<LspHover>;

    fn extract_url(params: &Self::Params) -> Option<Url> {
        Some(
            params
                .text_document_position_params
                .text_document
                .uri
                .clone(),
        )
    }

    fn modify_params(state: &ServerState, document: &Document, params: &mut Self::Params) {
        modify_incoming_position(
            state,
            document,
            &mut params.text_document_position_params.position,
        );
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let Some(hover) = response.as_mut()
            && let Some(range) = hover.range.as_mut()
        {
            modify_outgoing_range(state, document, range);
        }
    }
}

pub struct Completion;

impl Request for Completion {
    type Params = LspCompletionParams;
    type Response = Option<LspCompletionResponse>;

    fn extract_url(params: &Self::Params) -> Option<Url> {
        Some(params.text_document_position.text_document.uri.clone())
    }

    fn modify_params(state: &ServerState, document: &Document, params: &mut Self::Params) {
        modify_incoming_position(state, document, &mut params.text_document_position.position);
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let Some(response) = response.as_mut() {
            let items = match response {
                LspCompletionResponse::Array(v) => v,
                LspCompletionResponse::List(v) => v.items.as_mut(),
            };
            for item in items {
                if let Some(edit) = item.text_edit.as_mut() {
                    modify_outgoing_completion_text_edit(state, document, edit);
                }
                if let Some(edits) = item.additional_text_edits.as_mut() {
                    for edit in edits {
                        modify_outgoing_text_edit(state, document, edit);
                    }
                }
            }
        }
    }
}

pub struct CompletionResolve;

impl Request for CompletionResolve {
    type Params = LspCompletionItem;
    type Response = LspCompletionItem;

    // CompletionItem doesn't contain a document URI

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let Some(edit) = response.text_edit.as_mut() {
            modify_outgoing_completion_text_edit(state, document, edit);
        }

        if let Some(additional_edits) = response.additional_text_edits.as_mut() {
            for edit in additional_edits.iter_mut() {
                modify_outgoing_text_edit(state, document, edit);
            }
        }
    }
}

// ══════════════════════════
// Code Actions & Quick Fixes
// ══════════════════════════

pub struct CodeAction;

impl Request for CodeAction {
    type Params = LspCodeActionParams;
    type Response = Option<Vec<LspCodeActionOrCommand>>;

    fn extract_url(params: &Self::Params) -> Option<Url> {
        Some(params.text_document.uri.clone())
    }

    fn modify_params(state: &ServerState, document: &Document, params: &mut Self::Params) {
        modify_incoming_range(state, document, &mut params.range);
        for diag in &mut params.context.diagnostics {
            modify_incoming_diagnostic(state, document, diag);
        }
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let Some(actions) = response.as_mut() {
            for action in actions.iter_mut() {
                if let LspCodeActionOrCommand::CodeAction(action) = action {
                    if let Some(diagnostics) = action.diagnostics.as_mut() {
                        for diag in diagnostics {
                            modify_outgoing_diagnostic(state, document, diag);
                        }
                    }
                    if let Some(edit) = action.edit.as_mut() {
                        modify_outgoing_workspace_edit(state, document, edit);
                    }
                }
            }
        }
    }
}

pub struct CodeActionResolve;

impl Request for CodeActionResolve {
    type Params = LspCodeAction;
    type Response = LspCodeAction;

    // CodeAction doesn't contain a document URI

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let Some(diagnostics) = response.diagnostics.as_mut() {
            for diag in diagnostics {
                modify_outgoing_diagnostic(state, document, diag);
            }
        }
        if let Some(edit) = response.edit.as_mut() {
            modify_outgoing_workspace_edit(state, document, edit);
        }
    }
}

// ═══════════════════════════
// Document Links & Navigation
// ═══════════════════════════

pub struct DocumentLink;

impl Request for DocumentLink {
    type Params = LspDocumentLinkParams;
    type Response = Option<Vec<LspDocumentLink>>;

    fn extract_url(params: &Self::Params) -> Option<Url> {
        Some(params.text_document.uri.clone())
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let Some(links) = response.as_mut() {
            for link in links.iter_mut() {
                modify_outgoing_range(state, document, &mut link.range);
            }
        }
    }
}

pub struct DocumentLinkResolve;

impl Request for DocumentLinkResolve {
    type Params = LspDocumentLink;
    type Response = LspDocumentLink;

    fn extract_url(params: &Self::Params) -> Option<Url> {
        params.target.clone()
    }

    fn modify_params(state: &ServerState, document: &Document, params: &mut Self::Params) {
        modify_incoming_range(state, document, &mut params.range);
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        modify_outgoing_range(state, document, &mut response.range);
    }
}

// ══════════════════════════════
// Go-to Definition & Declaration
// ══════════════════════════════

pub struct Definition;

impl Request for Definition {
    type Params = LspGotoDefinitionParams;
    type Response = Option<LspGotoDefinitionResponse>;

    fn extract_url(params: &Self::Params) -> Option<Url> {
        Some(
            params
                .text_document_position_params
                .text_document
                .uri
                .clone(),
        )
    }

    fn modify_params(state: &ServerState, document: &Document, params: &mut Self::Params) {
        modify_incoming_position(
            state,
            document,
            &mut params.text_document_position_params.position,
        );
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let Some(response) = response.as_mut() {
            match response {
                LspGotoDefinitionResponse::Scalar(loc) => {
                    modify_outgoing_location(state, document, loc);
                }
                LspGotoDefinitionResponse::Array(locations) => {
                    for loc in locations.iter_mut() {
                        modify_outgoing_location(state, document, loc);
                    }
                }
                LspGotoDefinitionResponse::Link(links) => {
                    for link in links.iter_mut() {
                        modify_outgoing_location_link(state, document, link);
                    }
                }
            }
        }
    }
}

pub struct Declaration;

impl Request for Declaration {
    type Params = LspGotoDeclarationParams;
    type Response = Option<LspGotoDeclarationResponse>;

    fn extract_url(params: &Self::Params) -> Option<Url> {
        Some(
            params
                .text_document_position_params
                .text_document
                .uri
                .clone(),
        )
    }

    fn modify_params(state: &ServerState, document: &Document, params: &mut Self::Params) {
        modify_incoming_position(
            state,
            document,
            &mut params.text_document_position_params.position,
        );
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let Some(response) = response.as_mut() {
            match response {
                LspGotoDeclarationResponse::Scalar(loc) => {
                    modify_outgoing_location(state, document, loc);
                }
                LspGotoDeclarationResponse::Array(locations) => {
                    for loc in locations.iter_mut() {
                        modify_outgoing_location(state, document, loc);
                    }
                }
                LspGotoDeclarationResponse::Link(links) => {
                    for link in links.iter_mut() {
                        modify_outgoing_location_link(state, document, link);
                    }
                }
            }
        }
    }
}

// ══════════════════════════════
// References & Symbol Operations
// ══════════════════════════════

pub struct References;

impl Request for References {
    type Params = LspReferenceParams;
    type Response = Option<Vec<LspLocation>>;

    fn extract_url(params: &Self::Params) -> Option<Url> {
        Some(params.text_document_position.text_document.uri.clone())
    }

    fn modify_params(state: &ServerState, document: &Document, params: &mut Self::Params) {
        modify_incoming_position(state, document, &mut params.text_document_position.position);
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let Some(locations) = response.as_mut() {
            for loc in locations.iter_mut() {
                modify_outgoing_location(state, document, loc);
            }
        }
    }
}

pub struct Rename;

impl Request for Rename {
    type Params = LspRenameParams;
    type Response = Option<LspWorkspaceEdit>;

    fn extract_url(params: &Self::Params) -> Option<Url> {
        Some(params.text_document_position.text_document.uri.clone())
    }

    fn modify_params(state: &ServerState, document: &Document, params: &mut Self::Params) {
        modify_incoming_position(state, document, &mut params.text_document_position.position);
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let Some(response) = response.as_mut() {
            modify_outgoing_workspace_edit(state, document, response);
        }
    }
}

pub struct RenamePrepare;

impl Request for RenamePrepare {
    type Params = LspTextDocumentPositionParams;
    type Response = Option<LspPrepareRenameResponse>;

    fn extract_url(params: &Self::Params) -> Option<Url> {
        Some(params.text_document.uri.clone())
    }

    fn modify_params(state: &ServerState, document: &Document, params: &mut Self::Params) {
        modify_incoming_position(state, document, &mut params.position);
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let Some(response) = response.as_mut() {
            match response {
                LspPrepareRenameResponse::Range(range)
                | LspPrepareRenameResponse::RangeWithPlaceholder { range, .. } => {
                    modify_outgoing_range(state, document, range);
                }
                LspPrepareRenameResponse::DefaultBehavior { .. } => {}
            }
        }
    }
}

// ═══════════════════
// Formatting Requests
// ═══════════════════

pub struct DocumentFormat;

impl Request for DocumentFormat {
    type Params = LspDocumentFormattingParams;
    type Response = Option<Vec<LspTextEdit>>;

    fn extract_url(params: &Self::Params) -> Option<Url> {
        Some(params.text_document.uri.clone())
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let Some(edits) = response.as_mut() {
            for edit in edits.iter_mut() {
                modify_outgoing_text_edit(state, document, edit);
            }
        }
    }
}

pub struct DocumentRangeFormat;

impl Request for DocumentRangeFormat {
    type Params = LspDocumentRangeFormattingParams;
    type Response = Option<Vec<LspTextEdit>>;

    fn extract_url(params: &Self::Params) -> Option<Url> {
        Some(params.text_document.uri.clone())
    }

    fn modify_params(state: &ServerState, document: &Document, params: &mut Self::Params) {
        modify_incoming_range(state, document, &mut params.range);
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let Some(edits) = response.as_mut() {
            for edit in edits.iter_mut() {
                modify_outgoing_text_edit(state, document, edit);
            }
        }
    }
}

// ════════════════════
// Diagnostics Requests
// ════════════════════

pub struct DocumentDiagnostics;

impl Request for DocumentDiagnostics {
    type Params = DocumentDiagnosticParams;
    type Response = DocumentDiagnosticReportResult;

    fn extract_url(params: &Self::Params) -> Option<Url> {
        Some(params.text_document.uri.clone())
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(report)) =
            response
        {
            for diag in &mut report.full_document_diagnostic_report.items {
                modify_outgoing_diagnostic(state, document, diag);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use async_lsp::{
        ClientSocket,
        lsp_types::{
            CodeActionContext, CodeActionParams, CompletionItem, CompletionResponse, Diagnostic,
            DidOpenTextDocumentParams, GotoDefinitionResponse, Location, PartialResultParams,
            Position, Range, TextDocumentIdentifier, TextDocumentItem, TextEdit, Url,
            WorkDoneProgressParams, WorkspaceEdit,
        },
    };

    use crate::{server::Server, server_state::ServerState, text_utils::Encoding};

    use super::{CodeAction, Completion, Definition, Rename, Request};

    struct TestServer;

    impl Server for TestServer {}

    fn url(path: &str) -> Url {
        Url::parse(&format!("file:///tmp/{path}")).unwrap()
    }

    const fn p(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    const fn r(line: u32, start: u32, end: u32) -> Range {
        Range {
            start: p(line, start),
            end: p(line, end),
        }
    }

    fn open_document(state: &mut ServerState, uri: Url, text: impl Into<String>) {
        let _ = state.handle_document_open::<TestServer>(DidOpenTextDocumentParams {
            text_document: TextDocumentItem::new(uri, "test".into(), 1, text.into()),
        });
    }

    fn state_with_documents() -> (ServerState, Url, Url) {
        let mut state = ServerState::new::<TestServer>(ClientSocket::new_closed());
        state.set_position_encoding(Encoding::UTF16);

        let source = url("source.txt");
        let target = url("target.txt");
        open_document(&mut state, source.clone(), "abcdef");
        open_document(&mut state, target.clone(), "🙂abc");

        (state, source, target)
    }

    #[test]
    fn definition_locations_are_converted_using_their_own_document() {
        let (state, source, target) = state_with_documents();
        let document = state.document(&source).unwrap();
        let mut response = Some(GotoDefinitionResponse::Scalar(Location::new(
            target,
            r(0, 4, 4),
        )));

        <Definition as Request>::modify_response(&state, &document, &mut response);

        let Some(GotoDefinitionResponse::Scalar(loc)) = response else {
            panic!("expected scalar location");
        };
        assert_eq!(loc.range, r(0, 2, 2));
    }

    #[test]
    fn workspace_edits_are_converted_using_their_own_document() {
        let (state, source, target) = state_with_documents();
        let document = state.document(&source).unwrap();
        let mut response = Some(WorkspaceEdit {
            changes: Some(HashMap::from([(
                target,
                vec![TextEdit::new(r(0, 4, 4), "x".into())],
            )])),
            ..Default::default()
        });

        <Rename as Request>::modify_response(&state, &document, &mut response);

        let edit = response.unwrap();
        let edit = edit.changes.unwrap().into_values().next().unwrap();
        assert_eq!(edit[0].range, r(0, 2, 2));
    }

    #[test]
    fn completion_additional_text_edits_are_converted() {
        let (state, _, target) = state_with_documents();
        let document = state.document(&target).unwrap();
        let mut response = Some(CompletionResponse::Array(vec![CompletionItem {
            label: "item".into(),
            additional_text_edits: Some(vec![TextEdit::new(r(0, 4, 4), "x".into())]),
            ..Default::default()
        }]));

        <Completion as Request>::modify_response(&state, &document, &mut response);

        let Some(CompletionResponse::Array(items)) = response else {
            panic!("expected completion array");
        };
        assert_eq!(
            items[0].additional_text_edits.as_ref().unwrap()[0].range,
            r(0, 2, 2),
        );
    }

    #[test]
    fn code_action_context_diagnostics_are_converted() {
        let (state, _, target) = state_with_documents();
        let document = state.document(&target).unwrap();
        let mut params = CodeActionParams {
            text_document: TextDocumentIdentifier::new(target),
            range: r(0, 0, 2),
            context: CodeActionContext {
                diagnostics: vec![Diagnostic {
                    range: r(0, 2, 2),
                    message: "diagnostic".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };

        <CodeAction as Request>::modify_params(&state, &document, &mut params);

        assert_eq!(params.range, r(0, 0, 4));
        assert_eq!(params.context.diagnostics[0].range, r(0, 4, 4));
    }

    #[test]
    fn rename_edits_fall_back_to_request_document_when_target_is_unknown() {
        let (state, _, target) = state_with_documents();
        let document = state.document(&target).unwrap();
        let missing = url("missing.txt");
        let mut response = Some(WorkspaceEdit {
            changes: Some(HashMap::from([(
                missing,
                vec![TextEdit::new(r(0, 4, 4), "x".into())],
            )])),
            ..Default::default()
        });

        <Rename as Request>::modify_response(&state, &document, &mut response);

        let edit = response.unwrap();
        let edit = edit.changes.unwrap().into_values().next().unwrap();
        assert_eq!(edit[0].range, r(0, 2, 2));
    }
}
