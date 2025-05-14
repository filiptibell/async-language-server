#![allow(clippy::needless_pass_by_value)]

use async_lsp::ResponseError;
use thiserror::Error;

type BoxDynError = Box<dyn std::error::Error + Send + Sync + 'static>;

pub use async_lsp::ErrorCode as ServerErrorCode;

pub type ServerResult<T> = Result<T, ServerError>;

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("Failed to connect to port {0}")]
    TcpConnect(u16),
    #[error("Uncategorized error: {0}")]
    Unknown(String),
    #[error("JSON RPC error: {0}")]
    Rpc(ServerErrorCode, String),
    #[error(transparent)]
    Lsp(#[from] async_lsp::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl ServerError {
    pub fn unknown(error: impl Into<BoxDynError>) -> Self {
        ServerError::Unknown(error.into().to_string())
    }

    pub fn rpc(code: ServerErrorCode, message: impl ToString) -> Self {
        ServerError::Rpc(code, message.to_string())
    }
}

// From string-like errors to ServerError

impl From<String> for ServerError {
    fn from(error: String) -> Self {
        ServerError::Unknown(error)
    }
}

impl From<&String> for ServerError {
    fn from(error: &String) -> Self {
        ServerError::Unknown(error.clone())
    }
}

impl From<&str> for ServerError {
    fn from(error: &str) -> Self {
        ServerError::Unknown(error.to_string())
    }
}

impl From<BoxDynError> for ServerError {
    fn from(error: BoxDynError) -> Self {
        ServerError::Unknown(error.to_string())
    }
}

// From ServerError to the lsp ResponseError

impl From<ServerError> for ResponseError {
    fn from(value: ServerError) -> Self {
        if let ServerError::Rpc(code, message) = value {
            ResponseError::new(code, message)
        } else if let ServerError::Unknown(message) = value {
            ResponseError::new(ServerErrorCode::UNKNOWN_ERROR_CODE, message)
        } else {
            ResponseError::new(ServerErrorCode::INTERNAL_ERROR, value.to_string())
        }
    }
}
