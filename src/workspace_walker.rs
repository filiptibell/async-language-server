use std::{
    fs,
    path::{Path, PathBuf},
};

use async_lsp::lsp_types::Url;
use ignore::WalkBuilder;

use crate::{result::ServerError, server::ServerResult};

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceWalkConfig {
    include_hidden_files: bool,
    respect_ignore_files: bool,
}

impl WorkspaceWalkConfig {
    pub(crate) fn with_hidden_files(mut self, yes: bool) -> Self {
        self.include_hidden_files = yes;
        self
    }

    pub(crate) fn with_ignore_files(mut self, yes: bool) -> Self {
        self.respect_ignore_files = yes;
        self
    }
}

impl Default for WorkspaceWalkConfig {
    fn default() -> Self {
        Self {
            include_hidden_files: false,
            respect_ignore_files: true,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceWalker {
    roots: Vec<PathBuf>,
    config: WorkspaceWalkConfig,
}

impl WorkspaceWalker {
    pub(crate) fn new(roots: &[PathBuf], config: WorkspaceWalkConfig) -> ServerResult<Self> {
        let roots = roots
            .iter()
            .map(fs::canonicalize)
            .collect::<Result<_, _>>()?;

        Ok(Self { roots, config })
    }

    pub(crate) fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub(crate) fn files(&self) -> ServerResult<Vec<PathBuf>> {
        let mut files = Vec::new();

        for root in &self.roots {
            let mut builder = WalkBuilder::new(root);
            configure_walker(&mut builder, &self.config);

            for entry in builder.build() {
                let entry = entry.map_err(ServerError::unknown)?;
                if entry.file_type().is_some_and(|ty| ty.is_file()) {
                    files.push(entry.into_path());
                }
            }
        }

        files.sort();
        Ok(files)
    }
}

fn configure_walker(builder: &mut WalkBuilder, config: &WorkspaceWalkConfig) {
    builder
        .standard_filters(false)
        .hidden(!config.include_hidden_files)
        .parents(config.respect_ignore_files)
        .ignore(config.respect_ignore_files)
        .git_ignore(config.respect_ignore_files)
        .git_global(config.respect_ignore_files)
        .git_exclude(config.respect_ignore_files);
}

pub(crate) fn path_to_url(path: &Path) -> ServerResult<Url> {
    Url::from_file_path(path).map_err(|()| {
        ServerError::from(format!(
            "Failed to convert '{}' to a file URL",
            path.display()
        ))
    })
}
