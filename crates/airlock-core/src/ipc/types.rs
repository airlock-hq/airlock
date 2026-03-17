//! Core IPC types shared between daemon and app.

use serde::{Deserialize, Serialize};

/// Basic run information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunInfo {
    pub id: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,

    /// Derived from job/step results.
    pub status: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_sha: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_sha: Option<String>,

    /// Currently executing step name (for running pipelines).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step: Option<String>,

    pub created_at: i64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Information about a job execution result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResultInfo {
    pub id: String,

    /// Job key from workflow YAML (e.g., "lint", "test", "deploy").
    pub job_key: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    pub status: String,

    /// Topological order for display.
    pub job_order: i32,

    /// Unix timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,

    /// Unix timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Information about a pipeline step execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResultInfo {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub job_id: String,

    /// For convenience; duplicates the parent job's key.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub job_key: String,

    /// Step name (e.g., "lint", "test", "push", "create-pr").
    pub step: String,

    pub status: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,

    /// Duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Unix timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,

    /// Unix timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
}

/// Information about a generated artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInfo {
    pub name: String,

    /// Full path to the artifact file.
    pub path: String,

    /// Type of artifact (e.g., "json", "patch", "text").
    pub artifact_type: String,

    pub size_bytes: u64,

    /// Unix timestamp seconds (from file mtime).
    #[serde(default)]
    pub created_at: i64,
}

/// Result for the `approve_step` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApproveStepResult {
    pub run_id: String,
    pub job_key: String,
    pub step_name: String,
    pub success: bool,

    /// Should be "passed" after successful approval.
    pub new_step_status: String,

    /// Whether the pipeline resumed and completed.
    pub pipeline_completed: bool,

    /// Set if the pipeline is paused at another step awaiting approval.
    pub paused_at_step: Option<String>,
}

/// Result for the `apply_patches` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyPatchesResult {
    pub run_id: String,
    pub success: bool,
    pub applied_count: u32,

    /// New HEAD SHA after committing applied patches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_head_sha: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Per-patch errors (patches that failed to apply).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patch_errors: Vec<PatchError>,
}

/// Error for a specific patch that failed to apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchError {
    pub path: String,
    pub error: String,
}
