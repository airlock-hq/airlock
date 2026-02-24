//! Miscellaneous IPC operations.
//!
//! Includes health checks, sync operations, and other general methods.

use super::error::IpcError;
use super::types::{DaemonSyncAllResult, DaemonSyncResult};
use super::IpcClient;
use crate::HealthResponse;

impl IpcClient {
    /// Check daemon health
    pub async fn health(&self) -> Result<HealthResponse, IpcError> {
        let result = self.send_request("health", serde_json::json!({})).await?;
        let health: HealthResponse = serde_json::from_value(result)?;
        Ok(health)
    }

    /// Sync a repository with upstream
    pub async fn sync_repo(&self, repo_id: &str) -> Result<bool, IpcError> {
        let result = self
            .send_request("sync", serde_json::json!({ "repo_id": repo_id }))
            .await?;

        let daemon_result: DaemonSyncResult = serde_json::from_value(result)?;
        Ok(daemon_result.success)
    }

    /// Sync all repositories with upstream
    pub async fn sync_all(&self) -> Result<(u32, u32), IpcError> {
        let result = self.send_request("sync_all", serde_json::json!({})).await?;

        let daemon_result: DaemonSyncAllResult = serde_json::from_value(result)?;
        Ok((daemon_result.synced_count, daemon_result.failed_count))
    }
}
