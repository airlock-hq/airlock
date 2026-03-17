//! Configuration IPC types shared between daemon and app.

use serde::{Deserialize, Serialize};

/// Result for the `get_config` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetConfigResult {
    pub global: GlobalConfigInfo,

    /// Present if `repo_id` was provided and the repo exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<RepoConfigInfo>,
}

/// Result for the `update_config` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfigResult {
    pub success: bool,
    pub global_updated: bool,
    pub repo_updated: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_config_path: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_config_path: Option<String>,
}

/// Global configuration information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfigInfo {
    pub config_exists: bool,
    pub config_path: String,
    pub sync: SyncConfigInfo,
    pub storage: StorageConfigInfo,
    pub agent: AgentConfigInfo,
}

/// Sync configuration information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfigInfo {
    /// Whether to sync on fetch operations.
    pub on_fetch: bool,
}

/// Storage configuration information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfigInfo {
    /// Maximum age of artifacts in days.
    pub max_artifact_age_days: u32,
}

/// Agent configuration information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfigInfo {
    /// Which adapter is configured (e.g., "claude-code", "codex", "auto").
    pub adapter: String,
    pub model: Option<String>,
    pub max_turns: Option<u32>,
}

/// Repository-specific configuration information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfigInfo {
    pub repo_id: String,
    pub working_path: String,

    /// Whether `.airlock/workflows/` directory exists in the repo.
    pub config_exists: bool,

    /// Path to the workflows directory.
    pub config_path: String,

    /// Workflow files found in `.airlock/workflows/`.
    pub workflows: Vec<WorkflowFileInfo>,
}

/// Information about a workflow file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowFileInfo {
    /// Filename (e.g., "main.yml").
    pub filename: String,

    /// Display name from the workflow's `name:` field.
    pub name: Option<String>,
}

/// Updates to apply to global configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalConfigUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<SyncConfigUpdate>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<StorageConfigUpdate>,
}

/// Updates to apply to sync configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncConfigUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_fetch: Option<bool>,
}

/// Updates to apply to storage configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageConfigUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_artifact_age_days: Option<u32>,
}

/// Updates to apply to repository configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfigUpdate {
    pub repo_id: String,
}
