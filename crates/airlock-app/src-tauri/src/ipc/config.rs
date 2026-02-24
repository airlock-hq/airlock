//! Configuration IPC methods.
//!
//! Uses shared config types from airlock_core::ipc directly.

use super::error::IpcError;
use super::IpcClient;
use airlock_core::ipc::{
    GetConfigResult, GlobalConfigUpdate, RepoConfigUpdate, UpdateConfigResult,
};

impl IpcClient {
    /// Get configuration (global and optionally repo-specific)
    pub async fn get_config(&self, repo_id: Option<&str>) -> Result<GetConfigResult, IpcError> {
        let params = match repo_id {
            Some(id) => serde_json::json!({ "repo_id": id }),
            None => serde_json::json!({}),
        };

        let result = self.send_request("get_config", params).await?;
        Ok(serde_json::from_value(result)?)
    }

    /// Update configuration (global and/or repo-specific)
    pub async fn update_config(
        &self,
        global: Option<GlobalConfigUpdate>,
        repo: Option<RepoConfigUpdate>,
    ) -> Result<UpdateConfigResult, IpcError> {
        let mut params = serde_json::Map::new();
        if let Some(g) = global {
            params.insert("global".to_string(), serde_json::to_value(g)?);
        }
        if let Some(r) = repo {
            params.insert("repo".to_string(), serde_json::to_value(r)?);
        }

        let result = self
            .send_request("update_config", serde_json::Value::Object(params))
            .await?;
        Ok(serde_json::from_value(result)?)
    }
}
