//! Sync and storage configuration types.

use serde::{Deserialize, Serialize};

/// Configuration for the sync behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    /// Whether to sync on fetch operations.
    #[serde(default = "default_true")]
    pub on_fetch: bool,
}

fn default_true() -> bool {
    true
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self { on_fetch: true }
    }
}

/// Configuration for storage limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Maximum age of artifacts in days.
    #[serde(default = "default_artifact_age")]
    pub max_artifact_age_days: u32,
}

fn default_artifact_age() -> u32 {
    30
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            max_artifact_age_days: default_artifact_age(),
        }
    }
}
