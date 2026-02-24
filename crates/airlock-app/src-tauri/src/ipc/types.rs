//! Daemon response types for IPC deserialization.
//!
//! Types that match the daemon's IPC response format.
//! Shared types are re-exported from airlock_core::ipc.

use airlock_core::ipc::{ArtifactInfo, JobResultInfo, RunInfo, StepResultInfo};
use serde::Deserialize;

// =============================================================================
// Internal types for JSON-RPC protocol
// =============================================================================

/// JSON-RPC 2.0 request
#[derive(Debug, serde::Serialize)]
pub(crate) struct Request {
    pub jsonrpc: &'static str,
    pub method: &'static str,
    pub params: serde_json::Value,
    pub id: u32,
}

/// JSON-RPC 2.0 response
#[derive(Debug, Deserialize)]
pub(crate) struct Response {
    pub result: Option<serde_json::Value>,
    pub error: Option<RpcError>,
}

/// JSON-RPC 2.0 error
#[derive(Debug, Deserialize)]
pub(crate) struct RpcError {
    pub code: i32,
    pub message: String,
}

// =============================================================================
// Daemon-specific response types (not shared — daemon has extra fields)
// =============================================================================

#[derive(Debug, Deserialize)]
pub(crate) struct DaemonRepoInfo {
    pub id: String,
    pub working_path: String,
    pub upstream_url: String,
    pub gate_path: String,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DaemonSyncInfo {
    pub synced_at: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DaemonStatusResult {
    pub repo: DaemonRepoInfo,
    pub pending_runs: u32,
    pub latest_run: Option<RunInfo>,
    pub last_sync: Option<DaemonSyncInfo>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DaemonGetRunsResult {
    pub runs: Vec<RunInfo>,
}

/// Repository with status (matching daemon's RepoWithStatus)
#[derive(Debug, Deserialize)]
pub(crate) struct DaemonRepoWithStatus {
    pub id: String,
    pub working_path: String,
    pub upstream_url: String,
    pub gate_path: String,
    pub created_at: i64,
    pub pending_runs: u32,
    pub last_sync: Option<i64>,
}

/// Result for get_repos (matching daemon's GetReposResult)
#[derive(Debug, Deserialize)]
pub(crate) struct DaemonGetReposResult {
    pub repos: Vec<DaemonRepoWithStatus>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DaemonRunDetailInfo {
    pub id: String,
    pub repo_id: String,
    pub status: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub base_sha: Option<String>,
    #[serde(default)]
    pub head_sha: Option<String>,
    #[serde(default)]
    pub current_step: Option<String>,
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DaemonRunDetailResult {
    pub run: DaemonRunDetailInfo,
    #[serde(default)]
    pub jobs: Vec<JobResultInfo>,
    #[serde(default)]
    pub step_results: Vec<StepResultInfo>,
    #[serde(default)]
    pub artifacts: Vec<ArtifactInfo>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DaemonSyncResult {
    pub success: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DaemonSyncAllResult {
    pub synced_count: u32,
    pub failed_count: u32,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DaemonUpdateIntentDescriptionResult {
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DaemonReprocessRunResult {
    pub success: bool,
}
