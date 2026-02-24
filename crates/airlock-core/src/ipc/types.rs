//! Core IPC types shared between daemon and app.

use serde::{Deserialize, Serialize};

/// Basic run information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunInfo {
    /// Run ID.
    pub id: String,

    /// Repository ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,

    /// Run status (derived from job/step results).
    pub status: String,

    /// Branch being pushed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,

    /// Base commit SHA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,

    /// Head commit SHA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,

    /// Currently executing step name (for running pipelines).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step: Option<String>,

    /// When the run started.
    pub created_at: i64,

    /// When the run was last updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,

    /// When the run completed (if completed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,

    /// Error message if the run failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Information about a job execution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResultInfo {
    /// Job result ID.
    pub id: String,

    /// Job key (from workflow YAML, e.g., "lint", "test", "deploy").
    pub job_key: String,

    /// Job display name (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Job execution status.
    pub status: String,

    /// Topological order for display.
    pub job_order: i32,

    /// When the job started (Unix timestamp).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,

    /// When the job completed (Unix timestamp).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,

    /// Error message if the job failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Information about a pipeline step execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResultInfo {
    /// Step result ID.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,

    /// Job ID this step belongs to.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub job_id: String,

    /// Job key this step belongs to (for convenience).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub job_key: String,

    /// Step name (e.g., "lint", "test", "push", "create-pr").
    pub step: String,

    /// Step execution status.
    pub status: String,

    /// Exit code of the step command (if executed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,

    /// Duration of the step in milliseconds (if available).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,

    /// Error message if the step failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// When the step started (Unix timestamp).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,

    /// When the step completed (Unix timestamp).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
}

/// Information about a generated artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInfo {
    /// Name of the artifact file.
    pub name: String,

    /// Full path to the artifact file.
    pub path: String,

    /// Type of artifact (e.g., "json", "patch", "text").
    pub artifact_type: String,

    /// Size of the artifact in bytes.
    pub size_bytes: u64,

    /// When the artifact was created (Unix timestamp seconds, from file mtime).
    #[serde(default)]
    pub created_at: i64,
}

/// Result for the `approve_step` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApproveStepResult {
    /// Run ID containing the approved step.
    pub run_id: String,

    /// Job key containing the approved step.
    pub job_key: String,

    /// Name of the step that was approved.
    pub step_name: String,

    /// Whether the approval was successful.
    pub success: bool,

    /// New status of the step (should be "passed").
    pub new_step_status: String,

    /// Whether the pipeline resumed and completed.
    pub pipeline_completed: bool,

    /// Whether the pipeline is paused at another step awaiting approval.
    pub paused_at_step: Option<String>,
}

/// Result for the `apply_patches` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyPatchesResult {
    /// Run ID.
    pub run_id: String,

    /// Whether the operation succeeded overall.
    pub success: bool,

    /// Number of patches successfully applied.
    pub applied_count: u32,

    /// New HEAD SHA after committing applied patches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_head_sha: Option<String>,

    /// Error message if the operation failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Per-patch errors (patches that failed to apply).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patch_errors: Vec<PatchError>,
}

/// Error for a specific patch that failed to apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchError {
    /// Path to the patch file.
    pub path: String,

    /// Error message.
    pub error: String,
}
