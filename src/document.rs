use std::{
    io::{Read, Result},
    sync::Arc,
};

use async_lsp::lsp_types::Url;
use ropey::Rope;

#[cfg(feature = "tree-sitter")]
use async_lsp::lsp_types::{Position, Range};

use crate::server::DocumentMatcher;

#[cfg(feature = "tree-sitter")]
use crate::{
    tree_sitter::{Language, Node, Query, QueryCursor, StreamingIterator, Tree},
    tree_sitter_utils::{lsp_position_to_ts_point, ts_range_to_lsp_range},
};

/**
    A document tracked by the language server, containing
    the URL, text, version, and language of the document.

    May be cloned somewhat cheaply to take a snapshot
    of the current state of the document.

    Not meant to be updated by external sources, only read,
    since the language server should be responsible for
    always keeping the document up-to-date when edits occur.

    # `tree-sitter`

    With the `tree-sitter` crate feature enabled, the document
    may also optionally store a [`tree_sitter::Language`] and
    a parsed [`tree_sitter::Tree`] for the document's text.

    If a `tree-sitter` language has been associated with the
    document, the respective tree will be parsed using the initial
    contents, and incrementally updated thereafter, transparently.
*/
#[derive(Debug, Clone)]
pub struct Document {
    pub(crate) uri: Url,
    pub(crate) text: Rope,
    pub(crate) version: i32,
    pub(crate) language: String,
    pub(crate) matcher: Option<Arc<DocumentMatcher>>,
    #[cfg(feature = "tree-sitter")]
    pub(crate) tree_sitter_lang: Option<Language>,
    #[cfg(feature = "tree-sitter")]
    pub(crate) tree_sitter_tree: Option<Tree>,
}

impl Document {
    /**
        Returns the URL of the document.
    */
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.uri
    }

    /**
        Returns the text of the document, as
        its underlying [`Rope`] representation.

        It is usually easier to use one of the several convenience
        methods that [`Document`] provides for accessing and searching
        through text, but this method exists as an escape hatch.
    */
    #[must_use]
    pub fn text(&self) -> &Rope {
        &self.text
    }

    /**
        Returns a reader over the full text in the document.
    */
    #[must_use]
    pub fn text_reader(&self) -> DocumentReader<'_> {
        DocumentReader {
            chunks: self.text.chunks(),
            current: None,
            current_offset: 0,
        }
    }

    /**
        Returns the full text of the document, as a string.

        When possible, prefer [`Document::text_reader`]
        for improved performance and less allocations.
    */
    #[must_use]
    pub fn text_contents(&self) -> String {
        self.text.to_string()
    }

    /**
        Returns the full text of the document, as a string.

        When possible, prefer [`Document::text_reader`]
        for improved performance and less allocations.
    */
    #[must_use]
    pub fn text_bytes(&self) -> Vec<u8> {
        self.text.bytes().collect()
    }

    /**
        Returns the version of the document.

        This number should be strictly increasing with
        each change to the document, including undo/redo.
    */
    #[must_use]
    pub fn version(&self) -> i32 {
        self.version
    }

    /**
        Returns the language of the document.
    */
    #[must_use]
    pub fn language(&self) -> &str {
        &self.language
    }

    /**
        Returns the name of the document matcher that this document
        was matched against, if one was configured, and either a
        language or glob pattern was matched against.

        See [`DocumentMatcher`] for more information.
    */
    #[must_use]
    pub fn matched_name(&self) -> Option<&str> {
        self.matcher.as_ref().map(|matcher| matcher.name.as_str())
    }
}

#[cfg(feature = "tree-sitter")]
impl Document {
    /**
        Returns `true` if the document has an assigned tree-sitter language, otherwise `false`.
    */
    #[must_use]
    pub fn has_syntax_language(&self) -> bool {
        self.tree_sitter_lang.is_some()
    }

    /**
        Returns `true` if the document has a parsed tree-sitter syntax tree, otherwise `false`.
    */
    #[must_use]
    pub fn has_syntax_tree(&self) -> bool {
        self.tree_sitter_tree.is_some()
    }

    /**
        Returns the UTF-8 text of a [`Node`].

        Panics if the node is not valid for the document.
    */
    #[must_use]
    pub fn node_text(&self, node: Node) -> String {
        self.text.byte_slice(node.byte_range()).to_string()
    }

    /**
        Returns a [`Node`] at the root of the syntax tree, if one exists.
    */
    #[must_use]
    pub fn node_at_root(&self) -> Option<Node<'_>> {
        self.tree_sitter_tree.as_ref().map(|tree| tree.root_node())
    }

    /**
        Returns a [`Node`] at the given LSP position, if one exists.
    */
    #[must_use]
    pub fn node_at_position(&self, position: Position) -> Option<Node<'_>> {
        let root = self.node_at_root()?;
        let point = lsp_position_to_ts_point(position);
        root.descendant_for_point_range(point, point)
    }

    /**
        Similar to [`node_at_position`], except the node must be named.
    */
    #[must_use]
    pub fn node_at_position_named(&self, position: Position) -> Option<Node<'_>> {
        let root = self.node_at_root()?;
        let point = lsp_position_to_ts_point(position);
        root.named_descendant_for_point_range(point, point)
    }

    /**
        Creates and runs a query for the given query string.

        Returns `Some(captures)` if the query was successful, otherwise `None`.
    */
    #[must_use]
    pub fn query(&self, query: impl AsRef<str>) -> Option<Vec<DocumentQueryCapture>> {
        let lang = self.tree_sitter_lang.as_ref()?;
        let tree = self.tree_sitter_tree.as_ref()?;

        let query = Query::new(lang, query.as_ref()).ok()?;
        let query_names = query.capture_names();

        let doc_text = self.text.to_string();
        let doc_bytes = doc_text.as_bytes();

        let mut cursor = QueryCursor::new();
        let mut it = cursor.matches(&query, tree.root_node(), doc_bytes);

        let mut items = Vec::new();
        while let Some(matched) = it.next() {
            for capture in matched.captures {
                if let Ok(text) = capture.node.utf8_text(doc_bytes) {
                    let name = query_names[capture.index as usize].to_string();
                    let text = text.to_string();
                    let range = ts_range_to_lsp_range(capture.node.range());
                    items.push(DocumentQueryCapture { name, text, range });
                }
            }
        }
        Some(items)
    }
}

impl AsRef<Rope> for Document {
    fn as_ref(&self) -> &Rope {
        &self.text
    }
}

/**
    A reader over the full text contents of a document.

    Created by calling [`Document::text_reader`].
*/
pub struct DocumentReader<'d> {
    chunks: ropey::iter::Chunks<'d>,
    current: Option<&'d str>,
    current_offset: usize,
}

impl Read for DocumentReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut written = 0;

        while written < buf.len() {
            if self.current.is_none() {
                self.current = self.chunks.next();
                self.current_offset = 0;
            }

            let Some(chunk) = self.current else {
                break;
            };

            let remaining = &chunk.as_bytes()[self.current_offset..];
            let len = remaining.len().min(buf.len() - written);
            buf[written..written + len].copy_from_slice(&remaining[..len]);

            written += len;
            self.current_offset += len;

            if self.current_offset == chunk.len() {
                self.current = None;
                self.current_offset = 0;
            }
        }

        Ok(written)
    }
}

#[cfg(feature = "tree-sitter")]
/**
    A capture from a tree-sitter query on a document.

    Created by calling [`Document::query`].
*/
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentQueryCapture {
    /// The capture name
    pub name: String,
    /// The textual contents of the capture
    pub text: String,
    /// The document range of the capture
    pub range: Range,
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use ropey::Rope;

    use super::DocumentReader;

    #[test]
    fn reader_preserves_unread_chunk_bytes() {
        let text = Rope::from_str("hello");

        let mut reader = DocumentReader {
            chunks: text.chunks(),
            current: None,
            current_offset: 0,
        };

        let mut actual = Vec::new();
        let mut buf = [0; 1];
        while reader.read(&mut buf).unwrap() != 0 {
            actual.push(buf[0]);
        }

        assert_eq!(actual, b"hello");
    }
}
