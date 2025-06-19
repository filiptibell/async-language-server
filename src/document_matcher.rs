use std::{collections::HashMap, sync::Arc};

use async_lsp::lsp_types::Url;
use globset::{Glob, GlobSet};

#[cfg(feature = "tree-sitter")]
use tree_sitter::Language;

/**
    Options for matching documents based on their URLs and
    language identifiers, and associating them with an optional
    tree-sitter language grammar when tree-sitter feature is enabled.
*/
#[derive(Debug, Default, Clone)]
pub struct DocumentMatcher {
    /**
        The name of the document matcher.

        This may be used as a unique identifier for the matcher,
        and can be retrieved on documents using [`Document::matched_name`].
    */
    pub name: String,
    /**
        Optional globs to match documents based on their URLs.
    */
    pub url_globs: Vec<String>,
    /**
        Strings to match documents based on their language identifiers.
    */
    pub lang_strings: Vec<String>,
    #[cfg(feature = "tree-sitter")]
    /**
        The tree-sitter language grammar to associate with the matched document.
    */
    pub lang_grammar: Option<Language>,
}

impl DocumentMatcher {
    /**
        Creates a new document matcher with the given name.

        The name is only used for debugging purposes and will not be
        used for identifying documents. It does not need to be unique.
    */
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url_globs: Vec::new(),
            lang_strings: Vec::new(),
            #[cfg(feature = "tree-sitter")]
            lang_grammar: None,
        }
    }

    /**
        Adds the given URL globs to the matcher.
    */
    #[must_use]
    pub fn with_url_globs<I, U>(mut self, url_globs: I) -> Self
    where
        I: IntoIterator<Item = U>,
        U: Into<String>,
    {
        self.url_globs.extend(url_globs.into_iter().map(Into::into));
        self
    }

    /**
        Adds the given language identifiers to the matcher.
    */
    #[must_use]
    pub fn with_lang_strings<I, U>(mut self, lang_strings: I) -> Self
    where
        I: IntoIterator<Item = U>,
        U: Into<String>,
    {
        self.lang_strings
            .extend(lang_strings.into_iter().map(Into::into));
        self
    }

    #[cfg(feature = "tree-sitter")]
    /**
        Sets the tree-sitter language grammar
        to associate with the document matcher.
    */
    #[must_use]
    pub fn with_lang_grammar(mut self, lang_grammar: Language) -> Self {
        self.lang_grammar = Some(lang_grammar);
        self
    }
}

/**
    Private struct created from individual [`DocumentMatcher`]s
    to easily match against documents and find the original matcher.
*/
#[allow(dead_code)]
#[derive(Debug, Default, Clone)]
pub(crate) struct DocumentMatchers {
    globsets: Arc<Vec<(GlobSet, Arc<DocumentMatcher>)>>,
    languages: Arc<HashMap<String, Arc<DocumentMatcher>>>,
}

#[allow(dead_code)]
impl DocumentMatchers {
    pub(crate) fn new(it: impl IntoIterator<Item = DocumentMatcher>) -> Self {
        let mut globsets = Vec::new();
        let mut languages = HashMap::new();

        for matcher in it {
            let matcher = Arc::new(matcher);

            let mut globset = GlobSet::builder();
            let mut globset_any = false;
            for glob in &matcher.url_globs {
                if let Ok(glob) = Glob::new(glob) {
                    globset.add(glob);
                    globset_any = true;
                } else {
                    #[cfg(feature = "tracing")]
                    tracing::warn!(
                        "Encountered invalid glob pattern '{}' in matcher '{}'",
                        glob,
                        matcher.name
                    );
                }
            }

            if globset_any {
                if let Ok(globset) = globset.build() {
                    globsets.push((globset, Arc::clone(&matcher)));
                } else {
                    #[cfg(feature = "tracing")]
                    tracing::warn!("Encountered invalid globset in matcher '{}'", matcher.name);
                }
            }

            for lang in &matcher.lang_strings {
                let mut lang = lang.trim().to_string();
                lang.make_ascii_lowercase();
                languages.insert(lang, Arc::clone(&matcher));
            }
        }

        Self {
            globsets: Arc::new(globsets),
            languages: Arc::new(languages),
        }
    }

    pub(crate) fn find(&self, url: &Url, lang: &str) -> Option<Arc<DocumentMatcher>> {
        let mut lang = lang.trim().to_string();
        lang.make_ascii_lowercase();
        self.languages.get(lang.as_str()).cloned().or_else(|| {
            url.to_file_path().ok().and_then(|p| {
                self.globsets
                    .iter()
                    .find(|(globset, _)| globset.is_match(&p))
                    .map(|(_, matcher)| Arc::clone(matcher))
            })
        })
    }
}
