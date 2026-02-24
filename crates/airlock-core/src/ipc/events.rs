//! Event types for real-time updates shared between daemon and app.

use serde::{Deserialize, Serialize};

/// Events emitted by the daemon for real-time updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AirlockEvent {
    /// A new run was created.
    RunCreated {
        repo_id: String,
        run_id: String,
        branch: String,
    },
    /// A run's status was updated.
    RunUpdated {
        repo_id: String,
        run_id: String,
        status: String,
    },
    /// A job started executing.
    JobStarted {
        repo_id: String,
        run_id: String,
        job_key: String,
    },
    /// A job completed.
    JobCompleted {
        repo_id: String,
        run_id: String,
        job_key: String,
        status: String,
    },
    /// A step started executing.
    StepStarted {
        repo_id: String,
        run_id: String,
        job_key: String,
        step_name: String,
    },
    /// A step completed (passed, failed, or awaiting approval).
    StepCompleted {
        repo_id: String,
        run_id: String,
        job_key: String,
        step_name: String,
        status: String,
    },
    /// A pipeline run completed.
    RunCompleted {
        repo_id: String,
        run_id: String,
        success: bool,
    },
    /// A chunk of log output from a running step.
    LogChunk {
        repo_id: String,
        run_id: String,
        job_key: String,
        step_name: String,
        /// "stdout" or "stderr"
        stream: String,
        /// The log content (may be multiple lines)
        content: String,
    },
}
