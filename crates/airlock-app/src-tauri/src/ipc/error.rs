//! IPC error types.

use thiserror::Error;

/// Errors that can occur during IPC communication
#[derive(Error, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum IpcError {
    #[error("Failed to connect to daemon: {0}")]
    ConnectionError(String),

    #[error("Failed to send request: {0}")]
    SendError(String),

    #[error("Failed to receive response: {0}")]
    ReceiveError(String),

    #[error("JSON serialization error: {0}")]
    JsonError(String),

    #[error("RPC error ({code}): {message}")]
    RpcError { code: i32, message: String },

    #[error("IO error: {0}")]
    IoError(String),
}

impl From<serde_json::Error> for IpcError {
    fn from(e: serde_json::Error) -> Self {
        IpcError::JsonError(e.to_string())
    }
}

impl From<std::io::Error> for IpcError {
    fn from(e: std::io::Error) -> Self {
        IpcError::IoError(e.to_string())
    }
}
