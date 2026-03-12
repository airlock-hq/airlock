//! Error types for Airlock.

use thiserror::Error;

/// Result type alias for Airlock operations.
pub type Result<T> = std::result::Result<T, AirlockError>;

/// Error type for Airlock operations.
#[derive(Error, Debug)]
pub enum AirlockError {
    /// Wraps `git2::Error` and failures from shelling out to the `git` CLI.
    #[error("Git error: {0}")]
    Git(String),

    /// Wraps `rusqlite::Error` from the repos/runs/steps tables.
    #[error("Database error: {0}")]
    Database(String),

    /// Failures in the JSON-RPC transport between CLI and daemon.
    #[error("IPC error: {0}")]
    Ipc(String),

    /// YAML/JSON parsing failures for global config or workflow files.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Deprecated — use [`Agent`](Self::Agent) for all provider errors.
    #[error("LLM error: {0}")]
    Llm(String),

    /// Failures from agent adapters (Claude Code, Codex) including
    /// subprocess spawning, stream parsing, and provider API errors.
    #[error("Agent error: {0}")]
    Agent(String),

    /// First element is the resource kind (e.g. "Repo", "Run"), second is the identifier.
    #[error("{0} not found: {1}")]
    NotFound(String, String),

    /// The operation cannot proceed because a prerequisite state is missing
    /// (e.g. approving a step that isn't awaiting approval).
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Wraps `std::io::Error` for file/directory operations.
    #[error("Filesystem error: {0}")]
    Filesystem(String),

    /// The daemon socket doesn't exist or isn't responding.
    #[error("Daemon is not running")]
    DaemonNotRunning,

    /// The launchd/systemd service hasn't been registered yet.
    #[error("Service is not installed. Run 'airlock daemon install' first.")]
    ServiceNotInstalled,

    /// Failures during launchd/systemd service management (install, start, stop).
    #[error("Service error: {0}")]
    ServiceOperation(String),

    /// The requested feature isn't available on this OS or architecture.
    #[error("Unsupported: {0}")]
    Unsupported(String),

    /// Catch-all for errors that don't fit a more specific variant.
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
