//! Repository-related IPC methods.

use super::error::IpcError;
use super::types::{DaemonGetReposResult, DaemonStatusResult};
use super::IpcClient;
use crate::{RepoInfo, StatusResponse};

impl IpcClient {
    /// List all enrolled repositories
    pub async fn list_repos(&self) -> Result<Vec<RepoInfo>, IpcError> {
        let result = self
            .send_request("get_repos", serde_json::json!({}))
            .await?;

        let daemon_result: DaemonGetReposResult = serde_json::from_value(result)?;

        let repos = daemon_result
            .repos
            .into_iter()
            .map(|r| RepoInfo {
                id: r.id,
                working_path: r.working_path,
                upstream_url: r.upstream_url,
                gate_path: r.gate_path,
                created_at: r.created_at,
                last_sync: r.last_sync,
                pending_runs: r.pending_runs,
            })
            .collect();

        Ok(repos)
    }

    /// Get status for a specific repository
    pub async fn get_status(&self, repo_id: &str) -> Result<StatusResponse, IpcError> {
        let result = self
            .send_request("status", serde_json::json!({ "repo_id": repo_id }))
            .await?;

        let daemon_result: DaemonStatusResult = serde_json::from_value(result)?;

        Ok(StatusResponse {
            repo: RepoInfo {
                id: daemon_result.repo.id,
                working_path: daemon_result.repo.working_path,
                upstream_url: daemon_result.repo.upstream_url,
                gate_path: daemon_result.repo.gate_path,
                created_at: daemon_result.repo.created_at,
                last_sync: daemon_result.last_sync.map(|s| s.synced_at),
                pending_runs: daemon_result.pending_runs,
            },
            pending_runs: daemon_result.pending_runs,
            latest_run: daemon_result.latest_run,
        })
    }
}
