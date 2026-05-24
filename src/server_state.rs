#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::too_many_lines)]

use std::{collections::HashSet, ops::ControlFlow, path::PathBuf, sync::Arc};

use async_lsp::{
    ClientSocket, Result,
    lsp_types::{
        DidChangeTextDocumentParams, DidChangeWorkspaceFoldersParams, DidCloseTextDocumentParams,
        DidOpenTextDocumentParams, DidSaveTextDocumentParams, Url, WorkspaceFolder,
    },
};
use dashmap::DashMap;
use ropey::Rope;

#[cfg(feature = "tree-sitter")]
use tree_sitter::{InputEdit, Parser, Point};

use crate::{
    document::Document,
    document_matcher::DocumentMatchers,
    result::ServerResult,
    server::Server,
    text_utils::{Encoding, position_to_encoding},
    workspace_walker::{WorkspaceWalkConfig, WorkspaceWalker, path_to_url},
};

/**
    Managed state for an LSP server.

    Provides access to and automatically tracks the connected
    client, as well as opened documents and their changes.
*/
#[derive(Debug, Clone)]
pub struct ServerState {
    client: ClientSocket,
    documents: Arc<DashMap<Url, DocumentEntry>>,
    workspace_roots: Arc<DashMap<Url, PathBuf>>,
    #[allow(dead_code)]
    matchers: DocumentMatchers,
    encoding: Arc<Encoding>,
}

#[derive(Debug, Clone)]
struct DocumentEntry {
    document: Document,
    origin: DocumentOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocumentOrigin {
    Open,
    Workspace,
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
        let entry = self.documents.get(url)?;
        Some(entry.document.clone())
    }

    /**
        Gets snapshots of all documents currently tracked by the server.

        Each document is returned exactly as it was at the time of
        calling this method, just like [`ServerState::document`].
    */
    #[must_use]
    pub fn documents(&self) -> Vec<Document> {
        self.documents
            .iter()
            .map(|entry| entry.document.clone())
            .collect()
    }
}

// Private implementation

impl ServerState {
    pub(crate) fn new<T: Server>(client: ClientSocket) -> Self {
        let documents = Arc::new(DashMap::new());
        let workspace_roots = Arc::new(DashMap::new());
        let matchers = DocumentMatchers::new(T::server_document_matchers());
        let encoding = Arc::new(Encoding::default());
        Self {
            client,
            documents,
            workspace_roots,
            matchers,
            encoding,
        }
    }

    #[allow(clippy::extra_unused_type_parameters)]
    fn insert_document<T: Server>(
        &self,
        url: Url,
        text: String,
        version: i32,
        language: String,
        origin: DocumentOrigin,
    ) {
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

        let matcher = self.matchers.find(&url, &language);

        self.documents.insert(
            url.clone(),
            DocumentEntry {
                document: Document {
                    uri: url,
                    text: Rope::from(text),
                    version,
                    language,
                    matcher,
                    #[cfg(feature = "tree-sitter")]
                    tree_sitter_lang,
                    #[cfg(feature = "tree-sitter")]
                    tree_sitter_tree,
                },
                origin,
            },
        );
    }

    pub(crate) fn set_workspace_folders(&self, folders: impl IntoIterator<Item = WorkspaceFolder>) {
        self.workspace_roots.clear();

        for folder in folders {
            if let Some(path) = workspace_folder_path(&folder) {
                self.workspace_roots.insert(folder.uri, path);
            }
        }
    }

    pub(crate) fn handle_workspace_folders_change(
        &self,
        params: DidChangeWorkspaceFoldersParams,
    ) -> ControlFlow<Result<()>> {
        let removed_roots: Vec<_> = params
            .event
            .removed
            .iter()
            .filter_map(workspace_folder_path)
            .collect();

        for folder in params.event.removed {
            self.workspace_roots.remove(&folder.uri);
        }
        self.remove_workspace_documents_in_roots(&removed_roots);

        for folder in params.event.added {
            if let Some(path) = workspace_folder_path(&folder) {
                self.workspace_roots.insert(folder.uri, path);
            }
        }

        ControlFlow::Continue(())
    }

    pub(crate) fn workspace_roots(&self) -> Vec<PathBuf> {
        let mut roots: Vec<_> = self
            .workspace_roots
            .iter()
            .map(|root| root.value().clone())
            .collect();
        roots.sort();
        roots
    }

    pub(crate) fn document_urls(&self) -> Vec<Url> {
        let mut urls: Vec<_> = self
            .documents
            .iter()
            .map(|entry| entry.document.uri.clone())
            .collect();
        urls.sort();
        urls
    }

    pub(crate) fn document_workspace_version(&self, url: &Url) -> Option<i64> {
        let entry = self.documents.get(url)?;
        match entry.origin {
            DocumentOrigin::Open => Some(i64::from(entry.document.version())),
            DocumentOrigin::Workspace => None,
        }
    }

    pub(crate) fn refresh_workspace_documents<T: Server>(&self) -> ServerResult<Vec<Url>> {
        let roots = self.workspace_roots();
        if roots.is_empty() {
            return Ok(self.document_urls());
        }

        let walker = WorkspaceWalker::new(&roots, WorkspaceWalkConfig::default())?;
        let mut urls = Vec::new();

        for path in walker.files()? {
            let uri = path_to_url(&path)?;
            let Some(matcher) = self.matchers.find_url(&uri) else {
                continue;
            };

            urls.push(uri.clone());
            if self
                .documents
                .get(&uri)
                .is_some_and(|entry| entry.origin == DocumentOrigin::Open)
            {
                continue;
            }

            let language = matcher
                .lang_strings
                .first()
                .cloned()
                .unwrap_or_else(|| matcher.name.to_ascii_lowercase());
            let text = std::fs::read_to_string(&path)?;
            self.insert_document::<T>(uri, text, 0, language, DocumentOrigin::Workspace);
        }

        let urls: HashSet<_> = urls.into_iter().collect();
        self.documents.retain(|url, entry| {
            entry.origin == DocumentOrigin::Open
                || !url_is_in_roots(url, &roots)
                || urls.contains(url)
        });

        let mut urls: Vec<_> = urls.into_iter().collect();
        urls.sort();
        Ok(urls)
    }

    fn remove_workspace_documents_in_roots(&self, roots: &[PathBuf]) {
        if roots.is_empty() {
            return;
        }

        self.documents.retain(|url, entry| {
            entry.origin == DocumentOrigin::Open || !url_is_in_roots(url, roots)
        });
    }

    pub(crate) fn get_position_encoding(&self) -> Encoding {
        *self.encoding
    }

    pub(crate) fn set_position_encoding(&mut self, kind: impl Into<Encoding>) {
        self.encoding = Arc::new(kind.into());
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
            DocumentOrigin::Open,
        );

        ControlFlow::Continue(())
    }

    pub(crate) fn handle_document_close<T: Server>(
        &self,
        params: DidCloseTextDocumentParams,
    ) -> ControlFlow<Result<()>> {
        let url = params.text_document.uri;
        let Some(entry) = self.documents.get(&url) else {
            return ControlFlow::Continue(());
        };

        let language = entry.document.language.clone();
        let roots = self.workspace_roots();
        let keep_as_workspace =
            self.matchers.find_url(&url).is_some() && url_is_in_roots(&url, &roots);
        drop(entry);

        if !keep_as_workspace {
            self.documents.remove(&url);
            return ControlFlow::Continue(());
        }

        if let Ok(text) = std::fs::read_to_string(url.path()) {
            self.insert_document::<T>(url, text, 0, language, DocumentOrigin::Workspace);
        } else {
            self.documents.remove(&url);
        }

        ControlFlow::Continue(())
    }

    pub(crate) fn handle_document_change<T: Server>(
        &mut self,
        params: DidChangeTextDocumentParams,
    ) -> ControlFlow<Result<()>> {
        let Some(mut entry) = self.documents.get_mut(&params.text_document.uri) else {
            return ControlFlow::Continue(());
        };

        entry.origin = DocumentOrigin::Open;
        let doc = &mut entry.document;
        doc.version = params.text_document.version;

        let encoding = self.encoding.as_ref();

        // Try to perform an incremental update on the document contents, using the changes
        let mut incremental_update_failed = false;
        #[cfg(feature = "tree-sitter")]
        let mut tree_sitter_incrementally_edited = false;

        for change in params.content_changes {
            let Some(range) = change.range else {
                doc.text = Rope::from_str(&change.text);

                #[cfg(feature = "tree-sitter")]
                {
                    let mut parser = doc_parser(doc);
                    doc.tree_sitter_tree = parser
                        .as_mut()
                        .and_then(|parser| parser.parse(doc.text_contents(), None));
                }

                continue;
            };

            // 1. Convert the LSP positions, using their arbitrary encoding,
            //    to what Ropey expects to use for its incremental updates
            let start_char_absolute = if let Ok(line_start_char_offset) =
                doc.text.try_line_to_char(range.start.line as usize)
            {
                let start =
                    position_to_encoding(doc.text(), range.start, encoding, Encoding::UTF32);
                line_start_char_offset + start.character as usize
            } else {
                incremental_update_failed = true;
                break;
            };
            let end_char_absolute = if let Ok(line_start_char_offset) =
                doc.text.try_line_to_char(range.end.line as usize)
            {
                let end = position_to_encoding(doc.text(), range.end, encoding, Encoding::UTF32);
                (line_start_char_offset + end.character as usize)
                    .max(start_char_absolute)
                    .min(doc.text.len_chars())
            } else {
                incremental_update_failed = true;
                break;
            };

            // 3. Perform incremental edit on the syntax tree as well, if enabled
            //    Note that we need to do this before updating the document contents
            #[cfg(feature = "tree-sitter")]
            if let Some(tree) = doc.tree_sitter_tree.as_mut() {
                // Compute some byte offsets based on the yet-to-be-changed rope
                let start_byte = doc.text.char_to_byte(start_char_absolute);
                let old_end_byte = doc.text.char_to_byte(end_char_absolute);
                let new_end_byte = start_byte + change.text.len();

                // Convert the start and old end positions to the correct encoding
                let start_position =
                    position_to_encoding(&doc.text, range.start, encoding, Encoding::UTF8);
                let old_end_position =
                    position_to_encoding(&doc.text, range.end, encoding, Encoding::UTF8);

                // Compute the new end point based on the contents of the edit
                let (new_end_row, new_end_col_bytes) = change.text.chars().fold(
                    (
                        start_position.line as usize,
                        start_position.character as usize,
                    ),
                    |(row, col_bytes), ch| {
                        if ch == '\n' {
                            (row + 1, 0)
                        } else {
                            (row, col_bytes + ch.len_utf8())
                        }
                    },
                );

                // Finally, apply the edit to incrementally update the syntax tree
                tree.edit(&InputEdit {
                    start_byte,
                    old_end_byte,
                    new_end_byte,
                    start_position: Point {
                        row: start_position.line as usize,
                        column: start_position.character as usize,
                    },
                    old_end_position: Point {
                        row: old_end_position.line as usize,
                        column: old_end_position.character as usize,
                    },
                    new_end_position: Point {
                        row: new_end_row,
                        column: new_end_col_bytes,
                    },
                });
                tree_sitter_incrementally_edited = true;
            }

            // 4. Finally, try to incrementally update the document contents
            if doc
                .text
                .try_remove(start_char_absolute..end_char_absolute)
                .is_err()
                || doc
                    .text
                    .try_insert(start_char_absolute, &change.text)
                    .is_err()
            {
                incremental_update_failed = true;
                break;
            }
        }

        // If the incremental update was successful, and we applied edits to the syntax
        // tree, we must finalize those changes by parsing using tree-sitter once again
        #[cfg(feature = "tree-sitter")]
        if !incremental_update_failed
            && tree_sitter_incrementally_edited
            && let Some(tree) = doc.tree_sitter_tree.as_ref()
        {
            let mut parser = doc_parser(doc).expect("has tree - must have parser");
            let updated_tree = parser.parse(doc.text_contents(), Some(tree));
            doc.tree_sitter_tree = updated_tree;
        }

        // If the incremental update failed, we will re-insert the entire file instead
        // Note that we must first drop the document reference to prevent a deadlock
        if incremental_update_failed {
            let uri = doc.uri.clone();
            let version = doc.version();
            let language = doc.language.clone();

            drop(entry);

            // NOTE: We must read the contents of the file synchronously
            // as the fallback here, since notification handlers are actually
            // synchronous both according to LSP spec and the async-lsp crate
            if let Ok(text) = std::fs::read_to_string(uri.path()) {
                self.insert_document::<T>(uri, text, version, language, DocumentOrigin::Open);
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
        let Some(mut entry) = self.documents.get_mut(&url) else {
            return ControlFlow::Continue(());
        };

        // NOTE: We must read the contents of the file synchronously
        // as the fallback here, since notification handlers are actually
        // synchronous both according to LSP spec and the async-lsp crate
        let text = if let Some(text) = &params.text {
            Rope::from_str(text)
        } else if let Ok(text) = std::fs::read_to_string(url.path()) {
            Rope::from_str(&text)
        } else {
            drop(entry);
            self.documents.remove(&url);
            return ControlFlow::Continue(());
        };
        let doc = &mut entry.document;
        doc.text = text;

        // The implementor may want to know what, if any, document
        // matcher we may have matched against - so let's save that
        let matcher = self.matchers.find(doc.url(), doc.language());
        doc.matcher.clone_from(&matcher);

        // Since we just read the entire file contents, we will also
        // re-create the entire tree-sitter tree using those new contents
        #[cfg(feature = "tree-sitter")]
        {
            let mut tree_sitter_lang = matcher.and_then(|m| m.lang_grammar.clone());

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

fn url_is_in_roots(url: &Url, roots: &[PathBuf]) -> bool {
    url.to_file_path()
        .is_ok_and(|path| roots.iter().any(|root| path.starts_with(root)))
}

fn workspace_folder_path(folder: &WorkspaceFolder) -> Option<PathBuf> {
    let path = folder.uri.to_file_path().ok()?;
    Some(std::fs::canonicalize(&path).unwrap_or(path))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use async_lsp::{
        ClientSocket,
        lsp_types::{
            DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
            TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem, Url,
            VersionedTextDocumentIdentifier, WorkspaceFolder,
        },
    };

    use crate::server::{DocumentMatcher, Server};

    use super::ServerState;

    struct TestServer;

    impl Server for TestServer {
        fn server_document_matchers() -> Vec<DocumentMatcher> {
            vec![
                DocumentMatcher::new("Test")
                    .with_url_globs(["**/*.test", "*.test"])
                    .with_lang_strings(["test"]),
            ]
        }
    }

    fn url(path: &str) -> Url {
        Url::parse(&format!("file:///tmp/{path}")).unwrap()
    }

    fn open_document(state: &mut ServerState, uri: Url, text: impl Into<String>) {
        let _ = state.handle_document_open::<TestServer>(DidOpenTextDocumentParams {
            text_document: TextDocumentItem::new(uri, "test".into(), 1, text.into()),
        });
    }

    fn temp_workspace(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after epoch")
            .as_millis();
        let root =
            std::env::temp_dir().join(format!("async-language-server-state-{name}-{millis}"));
        fs::create_dir_all(&root).expect("temp workspace can be created");
        root
    }

    fn workspace_folder(path: &PathBuf) -> WorkspaceFolder {
        let uri = Url::from_file_path(path).expect("path can be converted to a URL");
        WorkspaceFolder {
            uri,
            name: "test".into(),
        }
    }

    #[test]
    fn full_content_change_replaces_document_text() {
        let mut state = ServerState::new::<TestServer>(ClientSocket::new_closed());
        let uri = url("full-change.txt");
        open_document(&mut state, uri.clone(), "old");

        let _ = state.handle_document_change::<TestServer>(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier::new(uri.clone(), 2),
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "new".into(),
            }],
        });

        assert_eq!(state.document(&uri).unwrap().text_contents(), "new");
        assert_eq!(state.document(&uri).unwrap().version(), 2);
    }

    #[test]
    fn workspace_documents_have_no_lsp_version() {
        let root = temp_workspace("workspace-version");
        let manifest = root.join("a.test");
        fs::write(&manifest, "disk").expect("test file can be written");

        let state = ServerState::new::<TestServer>(ClientSocket::new_closed());
        state.set_workspace_folders([workspace_folder(&root)]);
        let urls = state
            .refresh_workspace_documents::<TestServer>()
            .expect("workspace documents can be refreshed");

        assert_eq!(urls.len(), 1);
        assert_eq!(state.document(&urls[0]).unwrap().text_contents(), "disk");
        assert_eq!(state.document_workspace_version(&urls[0]), None);

        fs::remove_dir_all(root).expect("temp workspace can be removed");
    }

    #[test]
    fn workspace_refresh_preserves_open_documents() {
        let root = temp_workspace("open-document");
        let manifest = root.join("a.test");
        fs::write(&manifest, "disk").expect("test file can be written");
        let manifest = fs::canonicalize(manifest).expect("test file can be canonicalized");
        let uri = Url::from_file_path(&manifest).expect("path can be converted to a URL");

        let mut state = ServerState::new::<TestServer>(ClientSocket::new_closed());
        state.set_workspace_folders([workspace_folder(&root)]);
        open_document(&mut state, uri.clone(), "open");

        let urls = state
            .refresh_workspace_documents::<TestServer>()
            .expect("workspace documents can be refreshed");

        assert_eq!(urls, vec![uri.clone()]);
        assert_eq!(state.document(&uri).unwrap().text_contents(), "open");
        assert_eq!(state.document_workspace_version(&uri), Some(1));

        fs::remove_dir_all(root).expect("temp workspace can be removed");
    }

    #[test]
    fn closing_workspace_documents_keeps_disk_snapshot() {
        let root = temp_workspace("close-workspace-document");
        let manifest = root.join("a.test");
        fs::write(&manifest, "disk").expect("test file can be written");
        let manifest = fs::canonicalize(manifest).expect("test file can be canonicalized");
        let uri = Url::from_file_path(&manifest).expect("path can be converted to a URL");

        let mut state = ServerState::new::<TestServer>(ClientSocket::new_closed());
        state.set_workspace_folders([workspace_folder(&root)]);
        open_document(&mut state, uri.clone(), "open");

        let _ = state.handle_document_close::<TestServer>(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier::new(uri.clone()),
        });

        assert_eq!(state.document(&uri).unwrap().text_contents(), "disk");
        assert_eq!(state.document_workspace_version(&uri), None);

        fs::remove_dir_all(root).expect("temp workspace can be removed");
    }

    #[test]
    fn closing_non_workspace_documents_removes_them() {
        let root = temp_workspace("close-non-workspace-document");
        let manifest = root.join("a.test");
        fs::write(&manifest, "disk").expect("test file can be written");
        let manifest = fs::canonicalize(manifest).expect("test file can be canonicalized");
        let uri = Url::from_file_path(manifest).expect("path can be converted to a URL");

        let mut state = ServerState::new::<TestServer>(ClientSocket::new_closed());
        open_document(&mut state, uri.clone(), "open");

        let _ = state.handle_document_close::<TestServer>(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier::new(uri.clone()),
        });

        assert!(state.document(&uri).is_none());

        fs::remove_dir_all(root).expect("temp workspace can be removed");
    }
}
