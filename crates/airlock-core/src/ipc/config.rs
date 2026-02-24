//! Configuration IPC types shared between daemon and app.

use serde::{Deserialize, Serialize};

/// Result for the `get_config` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetConfigResult {
    /// Global configuration.
    pub global: GlobalConfigInfo,

    /// Repository-specific configuration (if repo_id was provided and repo exists).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<RepoConfigInfo>,
}

/// Result for the `update_config` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfigResult {
    /// Whether the update was successful.
    pub success: bool,

    /// Whether global config was updated.
    pub global_updated: bool,

    /// Whether repo config was updated.
    pub repo_updated: bool,

    /// Path to the global config file (if updated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_config_path: Option<String>,

    /// Path to the repo config file (if updated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_config_path: Option<String>,
}

/// Global configuration information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfigInfo {
    /// Whether the global config file exists.
    pub config_exists: bool,

    /// Path to the global config file.
    pub config_path: String,

    /// Sync configuration.
    pub sync: SyncConfigInfo,

    /// Storage configuration.
    pub storage: StorageConfigInfo,

    /// Agent configuration.
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

    /// Model override (if set).
    pub model: Option<String>,

    /// Max turns per invocation (if set).
    pub max_turns: Option<u32>,
}

/// Repository-specific configuration information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfigInfo {
    /// Repository ID.
    pub repo_id: String,

    /// Path to the repo working directory.
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
    /// Sync configuration updates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<SyncConfigUpdate>,

    /// Storage configuration updates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<StorageConfigUpdate>,
}

/// Updates to apply to sync configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncConfigUpdate {
    /// Whether to sync on fetch operations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_fetch: Option<bool>,
}

/// Updates to apply to storage configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageConfigUpdate {
    /// Maximum age of artifacts in days.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_artifact_age_days: Option<u32>,
}

/// Updates to apply to repository configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfigUpdate {
    /// Repository ID to update.
    pub repo_id: String,
}
