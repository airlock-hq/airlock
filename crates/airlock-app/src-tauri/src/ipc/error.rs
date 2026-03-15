//! IPC error types.

use thiserror::Error;

/// Errors that can occur during IPC communication.
#[derive(Error, Debug)]
pub enum IpcError {
    #[error("Failed to connect to daemon: {0}")]
    Connection(String),

    #[error("Failed to send request: {0}")]
    Send(String),

    #[error("Failed to receive response: {0}")]
    Receive(String),

    #[error("JSON serialization error: {0}")]
    Json(String),

    #[error("RPC error ({code}): {message}")]
    Rpc { code: i32, message: String },

    #[error("IO error: {0}")]
    Io(String),
}

impl From<serde_json::Error> for IpcError {
    fn from(e: serde_json::Error) -> Self {
        IpcError::Json(e.to_string())
    }
}

impl From<std::io::Error> for IpcError {
    fn from(e: std::io::Error) -> Self {
        IpcError::Io(e.to_string())
    }
}
