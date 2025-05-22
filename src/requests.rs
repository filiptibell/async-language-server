use async_lsp::lsp_types::{
    CodeAction as LspCodeAction, CodeActionOrCommand as LspCodeActionOrCommand,
    CodeActionParams as LspCodeActionParams, CompletionItem as LspCompletionItem,
    CompletionParams as LspCompletionParams, CompletionResponse as LspCompletionResponse,
    CompletionTextEdit as LspCompletionTextEdit,
    DocumentFormattingParams as LspDocumentFormattingParams, DocumentLink as LspDocumentLink,
    DocumentLinkParams as LspDocumentLinkParams,
    DocumentRangeFormattingParams as LspDocumentRangeFormattingParams,
    GotoDefinitionParams as LspGotoDefinitionParams,
    GotoDefinitionResponse as LspGotoDefinitionResponse, Hover as LspHover,
    HoverParams as LspHoverParams, Location as LspLocation, LocationLink as LspLocationLink, OneOf,
    Position as LspPosition, PrepareRenameResponse as LspPrepareRenameResponse,
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

fn modify_outgoing_position(state: &ServerState, document: &Document, position: &mut LspPosition) {
    *position = position_to_encoding(
        &document.text,
        *position,
        Encoding::UTF8,
        state.get_position_encoding(),
    );
}

fn modify_outgoing_location_link(
    state: &ServerState,
    document: &Document,
    link: &mut LspLocationLink,
) {
    if let Some(origin_range) = link.origin_selection_range.as_mut() {
        modify_outgoing_position(state, document, &mut origin_range.start);
        modify_outgoing_position(state, document, &mut origin_range.end);
    }

    modify_outgoing_position(state, document, &mut link.target_range.start);
    modify_outgoing_position(state, document, &mut link.target_range.end);

    modify_outgoing_position(state, document, &mut link.target_selection_range.start);
    modify_outgoing_position(state, document, &mut link.target_selection_range.end);
}

fn modify_outgoing_workspace_edit(
    state: &ServerState,
    document: &Document,
    edit: &mut LspWorkspaceEdit,
) {
    use async_lsp::lsp_types::DocumentChanges;

    let mut ranges = Vec::new();

    if let Some(changes) = edit.changes.as_mut() {
        for edits in changes.values_mut() {
            for text_edit in edits.iter_mut() {
                ranges.push(&mut text_edit.range);
            }
        }
    }

    if let Some(document_changes) = edit.document_changes.as_mut() {
        match document_changes {
            DocumentChanges::Edits(edits) => {
                for versioned_edit in edits.iter_mut() {
                    for text_edit in &mut versioned_edit.edits {
                        match text_edit {
                            OneOf::Left(l) => ranges.push(&mut l.range),
                            OneOf::Right(r) => ranges.push(&mut r.text_edit.range),
                        }
                    }
                }
            }
            DocumentChanges::Operations(ops) => {
                use async_lsp::lsp_types::DocumentChangeOperation;
                for op in ops.iter_mut() {
                    match op {
                        DocumentChangeOperation::Edit(edit) => {
                            for text_edit in &mut edit.edits {
                                match text_edit {
                                    OneOf::Left(l) => ranges.push(&mut l.range),
                                    OneOf::Right(r) => ranges.push(&mut r.text_edit.range),
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

    for range in ranges {
        modify_outgoing_position(state, document, &mut range.start);
        modify_outgoing_position(state, document, &mut range.end);
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
        if let Some(hover) = response.as_mut() {
            if let Some(range) = hover.range.as_mut() {
                modify_outgoing_position(state, document, &mut range.start);
                modify_outgoing_position(state, document, &mut range.end);
            }
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
                    match edit {
                        LspCompletionTextEdit::Edit(edit) => {
                            modify_outgoing_position(state, document, &mut edit.range.start);
                            modify_outgoing_position(state, document, &mut edit.range.end);
                        }
                        LspCompletionTextEdit::InsertAndReplace(edit) => {
                            modify_outgoing_position(state, document, &mut edit.insert.start);
                            modify_outgoing_position(state, document, &mut edit.insert.end);
                            modify_outgoing_position(state, document, &mut edit.replace.start);
                            modify_outgoing_position(state, document, &mut edit.replace.end);
                        }
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
            match edit {
                LspCompletionTextEdit::Edit(edit) => {
                    modify_outgoing_position(state, document, &mut edit.range.start);
                    modify_outgoing_position(state, document, &mut edit.range.end);
                }
                LspCompletionTextEdit::InsertAndReplace(edit) => {
                    modify_outgoing_position(state, document, &mut edit.insert.start);
                    modify_outgoing_position(state, document, &mut edit.insert.end);
                    modify_outgoing_position(state, document, &mut edit.replace.start);
                    modify_outgoing_position(state, document, &mut edit.replace.end);
                }
            }
        }

        if let Some(additional_edits) = response.additional_text_edits.as_mut() {
            for edit in additional_edits.iter_mut() {
                modify_outgoing_position(state, document, &mut edit.range.start);
                modify_outgoing_position(state, document, &mut edit.range.end);
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
        modify_incoming_position(state, document, &mut params.range.start);
        modify_incoming_position(state, document, &mut params.range.end);
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let Some(actions) = response.as_mut() {
            for action in actions.iter_mut() {
                if let LspCodeActionOrCommand::CodeAction(action) = action {
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
                modify_outgoing_position(state, document, &mut link.range.start);
                modify_outgoing_position(state, document, &mut link.range.end);
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
        modify_incoming_position(state, document, &mut params.range.start);
        modify_incoming_position(state, document, &mut params.range.end);
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        modify_outgoing_position(state, document, &mut response.range.start);
        modify_outgoing_position(state, document, &mut response.range.end);
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
                    modify_outgoing_position(state, document, &mut loc.range.start);
                    modify_outgoing_position(state, document, &mut loc.range.end);
                }
                LspGotoDefinitionResponse::Array(locations) => {
                    for loc in locations.iter_mut() {
                        modify_outgoing_position(state, document, &mut loc.range.start);
                        modify_outgoing_position(state, document, &mut loc.range.end);
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
                    modify_outgoing_position(state, document, &mut loc.range.start);
                    modify_outgoing_position(state, document, &mut loc.range.end);
                }
                LspGotoDeclarationResponse::Array(locations) => {
                    for loc in locations.iter_mut() {
                        modify_outgoing_position(state, document, &mut loc.range.start);
                        modify_outgoing_position(state, document, &mut loc.range.end);
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
                modify_outgoing_position(state, document, &mut loc.range.start);
                modify_outgoing_position(state, document, &mut loc.range.end);
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
                    modify_outgoing_position(state, document, &mut range.start);
                    modify_outgoing_position(state, document, &mut range.end);
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
                modify_outgoing_position(state, document, &mut edit.range.start);
                modify_outgoing_position(state, document, &mut edit.range.end);
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
        modify_incoming_position(state, document, &mut params.range.start);
        modify_incoming_position(state, document, &mut params.range.end);
    }

    fn modify_response(state: &ServerState, document: &Document, response: &mut Self::Response) {
        if let Some(edits) = response.as_mut() {
            for edit in edits.iter_mut() {
                modify_outgoing_position(state, document, &mut edit.range.start);
                modify_outgoing_position(state, document, &mut edit.range.end);
            }
        }
    }
}
