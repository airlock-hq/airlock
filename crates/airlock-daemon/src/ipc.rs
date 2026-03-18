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
    pub jsonrpc: String,
    pub method: String,
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
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    /// Can be null for notifications.
    #[serde(default)]
    pub id: serde_json::Value,
}

/// JSON-RPC 2.0 response.
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
    pub id: serde_json::Value,
}

/// JSON-RPC 2.0 error.
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
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
    // Standard codes
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    // Application-specific codes (-32000 to -32099)
    pub const REPO_NOT_FOUND: i32 = -32001;
    pub const RUN_NOT_FOUND: i32 = -32002;
    pub const INVALID_REPO_STATE: i32 = -32010;
    pub const GIT_ERROR: i32 = -32020;
    pub const DATABASE_ERROR: i32 = -32021;
    pub const STEP_NOT_FOUND: i32 = -32024;
    pub const JOB_NOT_RETRYABLE: i32 = -32025;
}

/// IPC method names.
pub mod methods {
    /// Client sends this to start receiving event notifications.
    pub const SUBSCRIBE: &str = "subscribe";
    pub const INIT: &str = "init";
    pub const EJECT: &str = "eject";
    pub const SYNC: &str = "sync";
    pub const SYNC_ALL: &str = "sync_all";
    pub const STATUS: &str = "status";
    pub const HEALTH: &str = "health";
    pub const GET_RUNS: &str = "get_runs";
    pub const GET_RUN_DETAIL: &str = "get_run_detail";
    /// Signals that a push stage has successfully forwarded changes to upstream.
    pub const MARK_FORWARDED: &str = "mark_forwarded";
    /// From post-receive hook.
    pub const PUSH_RECEIVED: &str = "push_received";
    /// From upload-pack wrapper; triggers sync-on-fetch logic.
    pub const FETCH_NOTIFICATION: &str = "fetch_notification";
    pub const SHUTDOWN: &str = "shutdown";
    pub const GET_REPOS: &str = "get_repos";
    pub const REPROCESS_RUN: &str = "reprocess_run";
    pub const GET_CONFIG: &str = "get_config";
    pub const UPDATE_CONFIG: &str = "update_config";
    pub const APPROVE_STEP: &str = "approve_step";
    pub const GET_RUN_DIFF: &str = "get_run_diff";
    pub const APPLY_PATCHES: &str = "apply_patches";
    pub const CANCEL_RUN: &str = "cancel_run";
    pub const RETRY_JOB: &str = "retry_job";
}

// =============================================================================
// Request parameter types
// =============================================================================

/// Parameters for the `init` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct InitParams {
    pub working_path: String,
}

/// Parameters for the `eject` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct EjectParams {
    pub working_path: String,
}

/// Parameters for the `sync` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncParams {
    pub repo_id: String,
}

/// Parameters for the `status` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusParams {
    pub repo_id: String,
}

/// Parameters for the `get_runs` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetRunsParams {
    pub repo_id: String,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Parameters for the `get_run_detail` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetRunDetailParams {
    pub run_id: String,
}

/// Parameters for the `mark_forwarded` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct MarkForwardedParams {
    pub run_id: String,
    pub ref_name: String,
    pub sha: String,
}

/// Parameters for the `push_received` notification (from post-receive hook).
#[derive(Debug, Serialize, Deserialize)]
pub struct PushReceivedParams {
    pub gate_path: String,
    pub ref_updates: Vec<RefUpdateParam>,
}

/// A single ref update parameter.
#[derive(Debug, Serialize, Deserialize)]
pub struct RefUpdateParam {
    pub ref_name: String,
    pub old_sha: String,
    pub new_sha: String,
}

/// Parameters for the `fetch_notification` method (from upload-pack wrapper).
#[derive(Debug, Serialize, Deserialize)]
pub struct FetchNotificationParams {
    pub gate_path: String,
}

/// Parameters for the `reprocess_run` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct ReprocessRunParams {
    pub run_id: String,
}

// =============================================================================
// Step-based pipeline parameters
// =============================================================================

/// Parameters for the `approve_step` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct ApproveStepParams {
    pub run_id: String,
    pub job_key: String,
    /// Must be in AwaitingApproval status.
    pub step_name: String,
}

/// Parameters for the `get_run_diff` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetRunDiffParams {
    pub run_id: String,
}

/// Parameters for the `apply_patches` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct ApplyPatchesParams {
    pub run_id: String,

    /// Required when multiple jobs may be paused concurrently.
    /// Falls back to the first AwaitingApproval job if omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_key: Option<String>,

    /// Paths to patch artifact JSON files.
    pub patch_paths: Vec<String>,
}

/// Parameters for the `cancel_run` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct CancelRunParams {
    pub run_id: String,
}

/// Parameters for the `retry_job` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct RetryJobParams {
    pub run_id: String,
    pub job_key: String,
}

/// Parameters for the `get_config` method.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct GetConfigParams {
    /// If omitted, only global configuration is returned.
    #[serde(default)]
    pub repo_id: Option<String>,
}

/// Parameters for the `update_config` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateConfigParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global: Option<GlobalConfigUpdate>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<RepoConfigUpdate>,
}

// =============================================================================
// Response result types
// =============================================================================

/// Result for the `init` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct InitResult {
    pub repo_id: String,
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
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub synced_at: i64,
}

/// Result for the `sync_all` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncAllResult {
    pub synced_count: u32,
    pub failed_count: u32,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub errors: Vec<SyncError>,
}

/// A sync error for a specific repo.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncError {
    pub repo_id: String,
    pub error: String,
}

/// Result for the `status` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResult {
    pub repo: RepoInfo,
    /// Running or pending approval.
    pub pending_runs: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_run: Option<RunInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync: Option<SyncInfo>,
}

/// Basic repository information.
#[derive(Debug, Serialize, Deserialize)]
pub struct RepoInfo {
    pub id: String,
    pub working_path: String,
    pub upstream_url: String,
    pub gate_path: String,
    pub created_at: i64,
}

/// Sync information.
#[derive(Debug, Serialize, Deserialize)]
pub struct SyncInfo {
    pub success: bool,
    pub synced_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Result for the `health` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResult {
    pub healthy: bool,
    pub version: String,
    pub repo_count: u32,
    pub database_ok: bool,
    pub socket_path: String,
}

/// Result for the `get_runs` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetRunsResult {
    pub runs: Vec<RunInfo>,
}

/// Result for the `get_run_detail` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetRunDetailResult {
    pub run: RunDetailInfo,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jobs: Vec<JobResultInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub step_results: Vec<StepResultInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactInfo>,
}

/// Detailed run information.
#[derive(Debug, Serialize, Deserialize)]
pub struct RunDetailInfo {
    pub id: String,
    pub repo_id: String,
    /// Derived from job/step results: running, completed, failed, awaiting_approval.
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub branch: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_sha: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub head_sha: String,
    /// Currently executing step name (for running pipelines).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_step: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub workflow_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ref_updates: Vec<RefUpdateParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
}

/// Result for the `push_received` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct PushReceivedResult {
    pub will_create_run: bool,
}

/// Result for the `mark_forwarded` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct MarkForwardedResult {
    pub success: bool,
}

/// Result for the `fetch_notification` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct FetchNotificationResult {
    pub synced: bool,
    /// Only meaningful if `synced` is true.
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Skipped because repo wasn't stale.
    pub skipped_not_stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_id: Option<String>,
}

/// Result for the `shutdown` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct ShutdownResult {
    pub acknowledged: bool,
}

/// Result for the `get_repos` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct GetReposResult {
    pub repos: Vec<RepoWithStatus>,
}

/// Repository information with current status.
#[derive(Debug, Serialize, Deserialize)]
pub struct RepoWithStatus {
    pub id: String,
    pub working_path: String,
    pub upstream_url: String,
    pub gate_path: String,
    pub created_at: i64,
    /// Running or pending approval.
    pub pending_runs: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_sync: Option<i64>,
    pub gate_healthy: bool,
}

/// Result for the `reprocess_run` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct ReprocessRunResult {
    pub run_id: String,
    pub success: bool,
    pub new_status: String,
}

/// Result for the `cancel_run` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct CancelRunResult {
    pub run_id: String,
    pub success: bool,
}

/// Result for the `retry_job` method.
#[derive(Debug, Serialize, Deserialize)]
pub struct RetryJobResult {
    pub run_id: String,
    pub job_key: String,
    pub success: bool,
    /// Job keys that were reset (target + downstream dependents).
    #[serde(default)]
    pub reset_jobs: Vec<String>,
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
