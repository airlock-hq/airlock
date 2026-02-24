//! Error types for Airlock.

use thiserror::Error;

/// Result type alias for Airlock operations.
pub type Result<T> = std::result::Result<T, AirlockError>;

/// Error type for Airlock operations.
#[derive(Error, Debug)]
pub enum AirlockError {
    /// Error during Git operations.
    #[error("Git error: {0}")]
    Git(String),

    /// Error during database operations.
    #[error("Database error: {0}")]
    Database(String),

    /// Error during IPC operations.
    #[error("IPC error: {0}")]
    Ipc(String),

    /// Error during configuration parsing.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Error during LLM API calls (deprecated, use Agent instead).
    #[error("LLM error: {0}")]
    Llm(String),

    /// Error during agent operations.
    #[error("Agent error: {0}")]
    Agent(String),

    /// Error when a requested resource is not found.
    #[error("{0} not found: {1}")]
    NotFound(String, String),

    /// Error when an operation is invalid in the current state.
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Error during filesystem operations.
    #[error("Filesystem error: {0}")]
    Filesystem(String),

    /// Error when the daemon is not running.
    #[error("Daemon is not running")]
    DaemonNotRunning,

    /// Error when the service is not installed.
    #[error("Service is not installed. Run 'airlock daemon install' first.")]
    ServiceNotInstalled,

    /// Error during service operations.
    #[error("Service error: {0}")]
    ServiceOperation(String),

    /// Error for unsupported operations on the current platform.
    #[error("Unsupported: {0}")]
    Unsupported(String),

    /// Catch-all for other errors.
    #[error("{0}")]
    Other(String),
}

impl From<std::io::Error> for AirlockError {
    fn from(err: std::io::Error) -> Self {
        AirlockError::Filesystem(err.to_string())
    }
}

impl From<serde_json::Error> for AirlockError {
    fn from(err: serde_json::Error) -> Self {
        AirlockError::Config(err.to_string())
    }
}

impl From<rusqlite::Error> for AirlockError {
    fn from(err: rusqlite::Error) -> Self {
        AirlockError::Database(err.to_string())
    }
}

impl From<git2::Error> for AirlockError {
    fn from(err: git2::Error) -> Self {
        AirlockError::Git(err.message().to_string())
    }
}
