//! Global configuration types.

use serde::{Deserialize, Serialize};

use super::sync::{StorageConfig, SyncConfig};

/// Global configuration (~/.airlock/config.yml).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalConfig {
    /// Sync configuration.
    #[serde(default)]
    pub sync: SyncConfig,

    /// Storage configuration.
    #[serde(default)]
    pub storage: StorageConfig,

    /// Agent configuration.
    #[serde(default)]
    pub agent: AgentGlobalConfig,
}

/// Agent adapter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentGlobalConfig {
    /// Which adapter to use: "claude-code", "codex", "auto".
    #[serde(default = "default_adapter")]
    pub adapter: String,

    /// Adapter-specific options.
    #[serde(default)]
    pub options: AgentOptions,
}

impl Default for AgentGlobalConfig {
    fn default() -> Self {
        Self {
            adapter: default_adapter(),
            options: AgentOptions::default(),
        }
    }
}

fn default_adapter() -> String {
    "auto".to_string()
}

/// Adapter-specific options.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentOptions {
    /// Model override.
    pub model: Option<String>,
    /// Max turns per invocation.
    pub max_turns: Option<u32>,
}
