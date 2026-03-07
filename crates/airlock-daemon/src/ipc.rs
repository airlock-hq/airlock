//! IPC types and handlers for JSON-RPC 2.0 communication.

use serde::{Deserialize, Serialize};

// Re-export shared types from airlock-core
pub use airlock_core::ipc::{
    AgentConfigInfo, AirlockEvent, ApplyPatchesResult, ApproveStepResult, ArtifactInfo,
    CommitDiffInfo, GetConfigResult, GetRunDiffResult, GlobalConfigInfo, GlobalConfigUpdate,
    JobResultInfo, PatchError, RepoConfigInfo, RepoConfigUpdate, RunInfo, StepResultInfo,
    StorageConfigInfo, SyncConfigInfo, UpdateConfigResult, WorkflowFileInfo,
};

/// JSON-RPC 2.0 notification (no id, no response expected).
#[derive(Debug, Serialize, Deserialize)]
pub struct Notification {
    /// JSON-RPC version (always "2.0").
    pub jsonrpc: String,

    /// Notification method name (e.g., "event").
    pub method: String,

    /// Notification parameters.
    pub params: serde_json::Value,
}

impl Notification {
    /// Create a new event notification.
    pub fn event(event: &AirlockEvent) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: "event".to_string(),
            params: serde_json::to_value(event).unwrap_or(serde_json::Value::Null),
        }
    }
}

/// JSON-RPC 2.0 request.
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    /// JSON-RPC version (always "2.0").
    pub jsonrpc: String,

    /// Request method name.
    pub method: String,

    /// Request parameters.
    #[serde(default)]
    pub params: serde_json::Value,

    /// Request ID (can be null for notifications).
    #[serde(default)]
    pub id: serde_json::Value,
}

/// JSON-RPC 2.0 response.
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    /// JSON-RPC version (always "2.0").
    pub jsonrpc: String,

    /// Result (present on success).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,

    /// Error (present on failure).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,

    /// Request ID.
    pub id: serde_json::Value,
}

/// JSON-RPC 2.0 error.
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcError {
    /// Error code.
    pub code: i32,

    /// Error message.
    pub message: String,

    /// Additional error data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl Response {
    /// Create a successful response.
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Create an error response.
    pub fn error(id: serde_json::Value, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(RpcError {
                code,
                message,
                data: None,
            }),
            id,
        }
    }
}

/// Standard JSON-RPC 2.0 error codes.
pub mod error_codes {
    /// Parse error.
    pub const PARSE_ERROR: i32 = -32700;

    /// Invalid request.
    pub const INVALID_REQUEST: i32 = -32600;

    /// Method not found.
    pub const METHOD_NOT_FOUND: i32 = -32601;

    /// Invalid params.
    pub const INVALID_PARAMS: i32 = -32602;

    /// Internal error.
    pub const INTERNAL_ERROR: i32 = -32603;

    // Application-specific error codes (range: -32000 to -32099)

    /// Repository not found.
    pub const REPO_NOT_FOUND: i32 = -32001;

    /// Run not found.
    pub const RUN_NOT_FOUND: i32 = -32002;

    /// Invalid repository state.
    pub const INVALID_REPO_STATE: i32 = -32010;

    /// Git operation failed.
    pub const GIT_ERROR: i32 = -32020;

    /// Database operation failed.
    pub const DATABASE_ERROR: i32 = -32021;

    /// Step not found.
    pub const STEP_NOT_FOUND: i32 = -32024;
}

/// IPC method names.
pub mod methods {
    /// Subscribe to real-time events.
    /// Client sends this to start receiving event notifications.
    pub const SUBSCRIBE: &str = "subscribe";

    /// Initialize Airlock in a repository.
    pub const INIT: &str = "init";

    /// Eject from Airlock (restore original git configuration).
    pub const EJECT: &str = "eject";

    /// Sync a repository with upstream.
    pub const SYNC: &str = "sync";

    /// Sync all repositories with upstream.
    pub const SYNC_ALL: &str = "sync_all";

    /// Get status of a repository.
    pub const STATUS: &str = "status";

    /// Health check.
    pub const HEALTH: &str = "health";

    /// Get runs for a repository.
    pub const GET_RUNS: &str = "get_runs";

    /// Get details of a specific run.
    pub const GET_RUN_DETAIL: &str = "get_run_detail";

    /// Signal that a push stage has successfully forwarded changes to upstream.
    pub const MARK_FORWARDED: &str = "mark_forwarded";

    /// Notification that a push was received (from post-receive hook).
    pub const PUSH_RECEIVED: &str = "push_received";

    /// Notification that a fetch was requested (from upload-pack wrapper).
    /// Triggers sync-on-fetch logic.
    pub const FETCH_NOTIFICATION: &str = "fetch_notification";

    /// Shutdown the daemon gracefully.
    pub const SHUTDOWN: &str = "shutdown";

    /// Get all enrolled repositories.
    pub const GET_REPOS: &str = "get_repos";

    /// Reprocess a run (re-run the full pipeline).
    pub const REPROCESS_RUN: &str = "reprocess_run";

    /// Get current LLM and repo configuration.
    pub const GET_CONFIG: &str = "get_config";

    /// Update LLM and repo configuration.
    pub const UPDATE_CONFIG: &str = "update_config";

    /// Approve a step that is awaiting approval.
    pub const APPROVE_STEP: &str = "approve_step";

    /// Get diff between base and head SHA for a run.
    pub const GET_RUN_DIFF: &str = "get_run_diff";

    /// Apply selected patches to a run's worktree.
    pub const APPLY_PATCHES: &str = "apply_patches";
}

// =============================================================================
// Request parameter types
// =============================================================================

/// Parameters for the `init` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct InitParams {
    /// Path to the working repository.
    pub working_path: String,
}

/// Parameters for the `eject` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct EjectParams {
    /// Path to the working repository.
    pub working_path: String,
}

/// Parameters for the `sync` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncParams {
    /// Repository ID to sync.
    pub repo_id: String,
}

/// Parameters for the `status` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusParams {
    /// Repository ID to get status for.
    pub repo_id: String,
}

/// Parameters for the `get_runs` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetRunsParams {
    /// Repository ID to get runs for.
    pub repo_id: String,

    /// Maximum number of runs to return.
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Parameters for the `get_run_detail` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetRunDetailParams {
    /// Run ID to get details for.
    pub run_id: String,
}

/// Parameters for the `mark_forwarded` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct MarkForwardedParams {
    /// Run ID that was forwarded.
    pub run_id: String,

    /// The ref that was pushed (e.g., "refs/heads/main").
    pub ref_name: String,

    /// The SHA that was pushed.
    pub sha: String,
}

/// Parameters for the `push_received` notification (from post-receive hook).
#[derive(Debug, Serialize, Deserialize)]
pub struct PushReceivedParams {
    /// Path to the gate repository.
    pub gate_path: String,

    /// Ref updates from the push.
    pub ref_updates: Vec<RefUpdateParam>,
}

/// A single ref update parameter.
#[derive(Debug, Serialize, Deserialize)]
pub struct RefUpdateParam {
    /// Ref name (e.g., refs/heads/main).
    pub ref_name: String,

    /// Old SHA.
    pub old_sha: String,

    /// New SHA.
    pub new_sha: String,
}

/// Parameters for the `fetch_notification` method (from upload-pack wrapper).
#[derive(Debug, Serialize, Deserialize)]
pub struct FetchNotificationParams {
    /// Path to the gate repository.
    pub gate_path: String,
}

/// Parameters for the `reprocess_run` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct ReprocessRunParams {
    /// Run ID to reprocess.
    pub run_id: String,
}

// =============================================================================
// Step-based pipeline parameters
// =============================================================================

/// Parameters for the `approve_step` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct ApproveStepParams {
    /// Run ID containing the step to approve.
    pub run_id: String,

    /// Key of the job containing the step.
    pub job_key: String,

    /// Name of the step to approve (must be in AwaitingApproval status).
    pub step_name: String,
}

/// Parameters for the `get_run_diff` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetRunDiffParams {
    /// Run ID to get diff for.
    pub run_id: String,
}

/// Parameters for the `apply_patches` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct ApplyPatchesParams {
    /// Run ID to apply patches to.
    pub run_id: String,

    /// Key of the job whose worktree should receive the patches.
    /// Required when multiple jobs may be paused concurrently.
    /// When omitted, falls back to the first AwaitingApproval job found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_key: Option<String>,

    /// Paths to the patch artifact JSON files to apply.
    pub patch_paths: Vec<String>,
}

/// Parameters for the `get_config` method.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct GetConfigParams {
    /// Optional repository ID to get repo-specific configuration.
    /// If not provided, only global configuration is returned.
    #[serde(default)]
    pub repo_id: Option<String>,
}

/// Parameters for the `update_config` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateConfigParams {
    /// Global configuration updates (if provided).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global: Option<GlobalConfigUpdate>,

    /// Repository-specific configuration updates (if provided).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<RepoConfigUpdate>,
}

// =============================================================================
// Response result types
// =============================================================================

/// Result for the `init` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct InitResult {
    /// Repository ID.
    pub repo_id: String,

    /// Path to the gate repository.
    pub gate_path: String,
}

/// Result for the `eject` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct EjectResult {
    /// Original upstream URL that was restored.
    pub upstream_url: String,
}

/// Result for the `sync` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncResult {
    /// Whether the sync succeeded.
    pub success: bool,

    /// Error message if sync failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Timestamp of the sync.
    pub synced_at: i64,
}

/// Result for the `sync_all` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncAllResult {
    /// Number of repos synced.
    pub synced_count: u32,

    /// Number of repos that failed to sync.
    pub failed_count: u32,

    /// Errors encountered (repo_id -> error message).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub errors: Vec<SyncError>,
}

/// A sync error for a specific repo.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncError {
    /// Repository ID.
    pub repo_id: String,

    /// Error message.
    pub error: String,
}

/// Result for the `status` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResult {
    /// Repository information.
    pub repo: RepoInfo,

    /// Number of pending runs (running or pending approval).
    pub pending_runs: u32,

    /// Latest run (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_run: Option<RunInfo>,

    /// Last sync information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync: Option<SyncInfo>,
}

/// Basic repository information.
#[derive(Debug, Serialize, Deserialize)]
pub struct RepoInfo {
    /// Repository ID.
    pub id: String,

    /// Path to the working repository.
    pub working_path: String,

    /// Upstream URL.
    pub upstream_url: String,

    /// Path to the gate repository.
    pub gate_path: String,

    /// When the repo was enrolled.
    pub created_at: i64,
}

/// Sync information.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncInfo {
    /// Whether the last sync succeeded.
    pub success: bool,

    /// When the last sync occurred.
    pub synced_at: i64,

    /// Error message if the last sync failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Result for the `health` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResult {
    /// Whether the daemon is healthy.
    pub healthy: bool,

    /// Daemon version.
    pub version: String,

    /// Number of enrolled repositories.
    pub repo_count: u32,

    /// Database status.
    pub database_ok: bool,

    /// Socket path.
    pub socket_path: String,
}

/// Result for the `get_runs` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetRunsResult {
    /// List of runs.
    pub runs: Vec<RunInfo>,
}

/// Result for the `get_run_detail` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetRunDetailResult {
    /// Run information.
    pub run: RunDetailInfo,

    /// Job results for this run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jobs: Vec<JobResultInfo>,

    /// Pipeline step results.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub step_results: Vec<StepResultInfo>,

    /// Generated artifacts for this run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactInfo>,
}

/// Detailed run information.
#[derive(Debug, Serialize, Deserialize)]
pub struct RunDetailInfo {
    /// Run ID.
    pub id: String,

    /// Repository ID.
    pub repo_id: String,

    /// Run status (derived from job/step results: running, completed, failed, awaiting_approval).
    pub status: String,

    /// Branch being pushed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub branch: String,

    /// Base commit SHA.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_sha: String,

    /// Head commit SHA.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub head_sha: String,

    /// Currently executing step name (for running pipelines).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_step: Option<String>,

    /// Workflow file that triggered this run.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub workflow_file: String,

    /// Workflow display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_name: Option<String>,

    /// Ref updates that triggered this run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ref_updates: Vec<RefUpdateParam>,

    /// Error message if the run failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// When the run started.
    pub created_at: i64,

    /// When the run was last updated.
    #[serde(default)]
    pub updated_at: i64,

    /// When the run completed (if completed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
}

/// Result for the `push_received` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct PushReceivedResult {
    /// Whether the push will create any pipeline runs.
    pub will_create_run: bool,
}

/// Result for the `mark_forwarded` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct MarkForwardedResult {
    /// Whether the bookkeeping succeeded.
    pub success: bool,
}

/// Result for the `fetch_notification` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct FetchNotificationResult {
    /// Whether a sync was performed.
    pub synced: bool,

    /// Whether the sync succeeded (only meaningful if synced is true).
    pub success: bool,

    /// Error message if the sync failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Whether sync was skipped because repo wasn't stale.
    pub skipped_not_stale: bool,

    /// The repo ID that was synced (if found).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
}

/// Result for the `shutdown` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct ShutdownResult {
    /// Whether the shutdown was acknowledged.
    pub acknowledged: bool,
}

/// Result for the `get_repos` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetReposResult {
    /// List of enrolled repositories with status.
    pub repos: Vec<RepoWithStatus>,
}

/// Repository information with current status.
#[derive(Debug, Serialize, Deserialize)]
pub struct RepoWithStatus {
    /// Repository ID.
    pub id: String,

    /// Path to the working repository.
    pub working_path: String,

    /// Upstream URL.
    pub upstream_url: String,

    /// Path to the gate repository.
    pub gate_path: String,

    /// When the repo was enrolled.
    pub created_at: i64,

    /// Number of pending runs (running or pending approval).
    pub pending_runs: u32,

    /// Last sync timestamp (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync: Option<i64>,

    /// Whether the gate path exists and is valid.
    pub gate_healthy: bool,
}

/// Result for the `reprocess_run` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct ReprocessRunResult {
    /// Run ID that was reprocessed.
    pub run_id: String,

    /// Whether the reprocessing started successfully.
    pub success: bool,

    /// New status of the run.
    pub new_status: String,
}

// =============================================================================
// Step-based pipeline results
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_success() {
        let response = Response::success(serde_json::json!(1), serde_json::json!({"status": "ok"}));

        assert_eq!(response.jsonrpc, "2.0");
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_response_error() {
        let response = Response::error(
            serde_json::json!(1),
            error_codes::METHOD_NOT_FOUND,
            "Method not found".to_string(),
        );

        assert_eq!(response.jsonrpc, "2.0");
        assert!(response.result.is_none());
        assert!(response.error.is_some());
        assert_eq!(
            response.error.as_ref().unwrap().code,
            error_codes::METHOD_NOT_FOUND
        );
    }

    #[test]
    fn test_parse_init_params() {
        let json = r#"{"working_path": "/home/user/project"}"#;
        let params: InitParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.working_path, "/home/user/project");
    }

    #[test]
    fn test_parse_push_received_params() {
        let json = r#"{
            "gate_path": "/home/user/.airlock/repos/abc123.git",
            "ref_updates": [
                {"ref_name": "refs/heads/main", "old_sha": "abc", "new_sha": "def"}
            ]
        }"#;
        let params: PushReceivedParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.gate_path, "/home/user/.airlock/repos/abc123.git");
        assert_eq!(params.ref_updates.len(), 1);
    }
}
