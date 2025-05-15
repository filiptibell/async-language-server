#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::too_many_lines)]

use std::{ops::ControlFlow, sync::Arc};

use dashmap::DashMap;
use ropey::Rope;

use async_lsp::{
    ClientSocket, Result,
    lsp_types::{
        DidChangeTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams, Url,
    },
};

#[cfg(feature = "tree-sitter")]
use tree_sitter::{InputEdit, Parser, Point};

use crate::{document::Document, document_matcher::DocumentMatchers, server::Server};

/**
    Managed state for an LSP server.

    Provides access to and automatically tracks the connected
    client, as well as opened documents and their changes.
*/
#[derive(Debug, Clone)]
pub struct ServerState {
    client: ClientSocket,
    documents: Arc<DashMap<Url, Document>>,
    #[allow(dead_code)]
    matchers: DocumentMatchers,
}

impl ServerState {
    /**
        Gets a handle to the client connected to the server.

        Can be used to send requests and notifications to the client.
    */
    #[must_use]
    pub fn client(&self) -> ClientSocket {
        self.client.clone()
    }

    /**
        Gets a snapshot of a document by its URL.

        This will return the document exactly as it was
        at the time of calling this method - any further
        modifications such as saves or edits will not be
        reflected in the returned document or its contents.

        Returns `None` if the document is not found.
    */
    #[must_use]
    pub fn document(&self, url: &Url) -> Option<Document> {
        let doc = self.documents.get(url)?;
        Some(doc.clone())
    }
}

// Private implementation

impl ServerState {
    pub(crate) fn new<T: Server>(client: ClientSocket) -> Self {
        let documents = Arc::new(DashMap::new());
        let matchers = DocumentMatchers::new(T::server_document_matchers());
        Self {
            client,
            documents,
            matchers,
        }
    }

    #[allow(clippy::extra_unused_type_parameters)]
    fn insert_document<T: Server>(&self, url: Url, text: String, version: i32, language: String) {
        #[cfg(feature = "tree-sitter")]
        let mut tree_sitter_lang = self
            .matchers
            .find(&url, language.as_str())
            .and_then(|m| m.lang_grammar.clone());

        #[cfg(feature = "tree-sitter")]
        let tree_sitter_tree = if let Some(lang) = tree_sitter_lang.as_ref() {
            let mut parser = Parser::new();
            if parser.set_language(lang).is_ok() {
                parser.parse(&text, None)
            } else {
                tree_sitter_lang.take();
                None
            }
        } else {
            None
        };

        self.documents.insert(
            url.clone(),
            Document {
                uri: url,
                text: Rope::from(text),
                version,
                language,
                #[cfg(feature = "tree-sitter")]
                tree_sitter_lang,
                #[cfg(feature = "tree-sitter")]
                tree_sitter_tree,
            },
        );
    }

    pub(crate) fn handle_document_open<T: Server>(
        &mut self,
        params: DidOpenTextDocumentParams,
    ) -> ControlFlow<Result<()>> {
        self.insert_document::<T>(
            params.text_document.uri,
            params.text_document.text,
            params.text_document.version,
            params.text_document.language_id,
        );

        ControlFlow::Continue(())
    }

    pub(crate) fn handle_document_change<T: Server>(
        &mut self,
        params: DidChangeTextDocumentParams,
    ) -> ControlFlow<Result<()>> {
        let Some(mut doc) = self.documents.get_mut(&params.text_document.uri) else {
            return ControlFlow::Continue(());
        };

        doc.version = params.text_document.version;

        // Try to perform an incremental update on the document contents, using the changes
        let mut incremental_update_failed = false;
        for change in params.content_changes {
            let Some(range) = change.range else { continue };

            // First, try to get the starting character index,
            // since without it, we can't incrementally update
            let start_char = if let Ok(lchar) = doc.text.try_line_to_char(range.start.line as usize)
            {
                lchar + range.start.character as usize
            } else {
                incremental_update_failed = true;
                break;
            };

            // Try to get the ending character index - if it fails, we will
            // consider the change as extending to the end of the document
            let end_char = doc
                .text
                .try_line_to_char(range.end.line as usize)
                .map(|lchar| lchar + range.end.character as usize)
                .and_then(|c| {
                    if c > doc.text.len_chars() {
                        Err(ropey::Error::CharIndexOutOfBounds(c, doc.text.len_chars()))
                    } else {
                        Ok(c)
                    }
                })
                .ok();

            // Perform incremental edit on the syntax tree as well, if enabled
            // Note that we need to do this before updating the document contents
            #[cfg(feature = "tree-sitter")]
            if doc.tree_sitter_tree.is_some() {
                // Compute some byte offsets based on the yet-to-be-changed rope
                let start_byte = doc.text.char_to_byte(start_char);
                let old_end_byte = doc
                    .text
                    .char_to_byte(end_char.unwrap_or_else(|| doc.text.len_chars()));
                let new_end_byte = start_byte + change.text.len();

                // Compute the new end point based on the contents of the edit
                let (new_end_row, new_end_col) = change.text.chars().fold(
                    (range.start.line as usize, range.start.character as usize),
                    |(row, col), ch| {
                        if ch == '\n' {
                            (row + 1, 0)
                        } else {
                            (row, col + 1)
                        }
                    },
                );

                // Old end position will be either the LSP `range.end`
                // or the last character that already existed in the rope
                let old_end_position = if end_char.is_some() {
                    Point {
                        row: range.end.line as usize,
                        column: range.end.character as usize,
                    }
                } else {
                    let last_row = doc.text.len_lines() - 1;
                    let last_col = doc.text.line(last_row).len_chars();
                    Point {
                        row: last_row,
                        column: last_col,
                    }
                };

                // Finally, apply the edit to incrementally update the syntax tree
                doc.tree_sitter_tree.as_mut().unwrap().edit(&InputEdit {
                    start_byte,
                    old_end_byte,
                    new_end_byte,
                    start_position: Point {
                        row: range.start.line as usize,
                        column: range.start.character as usize,
                    },
                    old_end_position,
                    new_end_position: Point {
                        row: new_end_row,
                        column: new_end_col,
                    },
                });
            }

            // Try to incrementally update the document contents
            if let Some(end_char) = end_char {
                if doc.text.try_remove(start_char..end_char).is_err()
                    || doc.text.try_insert(start_char, &change.text).is_err()
                {
                    incremental_update_failed = true;
                    break;
                }
            } else if doc.text.try_remove(start_char..).is_err()
                || doc.text.try_insert(start_char, &change.text).is_err()
            {
                incremental_update_failed = true;
                break;
            }
        }

        // If the incremental update was successful, and we applied edits to the syntax
        // tree, we must finalize those changes by parsing using tree-sitter once again
        #[cfg(feature = "tree-sitter")]
        if !incremental_update_failed {
            if let Some(tree) = doc.tree_sitter_tree.as_ref() {
                let mut parser = doc_parser(&doc).expect("has tree - must have parser");
                let updated_tree = parser.parse(doc.text_contents(), Some(tree));
                doc.tree_sitter_tree = updated_tree;
            }
        }

        // If the incremental update failed, we will re-insert the entire file instead
        // Note that we must first drop the document reference to prevent a deadlock
        if incremental_update_failed {
            let uri = doc.uri.clone();
            let version = doc.version();
            let language = doc.language.clone();

            drop(doc);

            // NOTE: We must read the contents of the file synchronously
            // as the fallback here, since notification handlers are actually
            // synchronous both according to LSP spec and the async-lsp crate
            if let Ok(text) = std::fs::read_to_string(uri.path()) {
                self.insert_document::<T>(uri, text, version, language);
            } else {
                self.documents.remove(&uri);
            }
        }

        ControlFlow::Continue(())
    }

    #[allow(clippy::extra_unused_type_parameters)]
    pub(crate) fn handle_document_save<T: Server>(
        &self,
        params: DidSaveTextDocumentParams,
    ) -> ControlFlow<Result<()>> {
        let url = params.text_document.uri;
        let Some(mut doc) = self.documents.get_mut(&url) else {
            return ControlFlow::Continue(());
        };

        // NOTE: We must read the contents of the file synchronously
        // as the fallback here, since notification handlers are actually
        // synchronous both according to LSP spec and the async-lsp crate
        doc.text = if let Some(text) = &params.text {
            Rope::from_str(text)
        } else if let Ok(text) = std::fs::read_to_string(url.path()) {
            Rope::from_str(&text)
        } else {
            self.documents.remove(&url);
            return ControlFlow::Continue(());
        };

        // Since we just read the entire file contents, we will also
        // re-create the entire tree-sitter tree using those new contents
        #[cfg(feature = "tree-sitter")]
        {
            let mut tree_sitter_lang = self
                .matchers
                .find(doc.url(), doc.language())
                .and_then(|m| m.lang_grammar.clone());

            let tree_sitter_tree = if let Some(lang) = tree_sitter_lang.as_ref() {
                let mut parser = Parser::new();
                if parser.set_language(lang).is_ok() {
                    parser.parse(doc.text_contents(), None)
                } else {
                    tree_sitter_lang.take();
                    None
                }
            } else {
                None
            };

            doc.tree_sitter_lang = tree_sitter_lang;
            doc.tree_sitter_tree = tree_sitter_tree;
        }

        ControlFlow::Continue(())
    }
}

#[cfg(feature = "tree-sitter")]
fn doc_parser(doc: &Document) -> Option<Parser> {
    let lang = doc.tree_sitter_lang.as_ref()?;
    let mut parser = Parser::new();
    if parser.set_language(lang).is_ok() {
        Some(parser)
    } else {
        None
    }
}
