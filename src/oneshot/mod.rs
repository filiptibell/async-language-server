use std::path::Path;

use async_lsp::lsp_types::Url;

use crate::{result::ServerError, server::ServerResult};

mod server;
mod walker;
mod workspace_diagnostics;

pub use workspace_diagnostics::{
    DocumentDiagnostics, WorkspaceDiagnosticConfig, WorkspaceDiagnosticReport,
    workspace_diagnostics,
};

fn path_to_url(path: &Path) -> ServerResult<Url> {
    Url::from_file_path(path).map_err(|()| {
        ServerError::from(format!(
            "Failed to convert '{}' to a file URL",
            path.display()
        ))
    })
}
