//! Config handlers.
//!
//! Handles getting and updating configuration.

use super::util::parse_params;
use super::HandlerContext;
use crate::ipc::{
    error_codes, AgentConfigInfo, GetConfigParams, GetConfigResult, GlobalConfigInfo,
    RepoConfigInfo, Response, StorageConfigInfo, SyncConfigInfo, UpdateConfigParams,
    UpdateConfigResult, WorkflowFileInfo,
};
use airlock_core::{load_global_config, load_workflows_from_disk, REPO_CONFIG_PATH};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Handle the `get_config` method.
///
/// Returns current configuration including workflow file information.
pub async fn handle_get_config(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    // Parse parameters (all optional)
    let params: GetConfigParams = serde_json::from_value(params).unwrap_or_default();

    // Load global configuration
    let global_config_path = ctx.paths.global_config();
    let global_config_exists = global_config_path.exists();

    let global_config = if global_config_exists {
        match load_global_config(&global_config_path) {
            Ok(config) => config,
            Err(e) => {
                warn!("Failed to load global config: {}", e);
                airlock_core::GlobalConfig::default()
            }
        }
    } else {
        airlock_core::GlobalConfig::default()
    };

    let global_info = GlobalConfigInfo {
        config_exists: global_config_exists,
        config_path: global_config_path.to_string_lossy().to_string(),
        sync: SyncConfigInfo {
            on_fetch: global_config.sync.on_fetch,
        },
        storage: StorageConfigInfo {
            max_artifact_age_days: global_config.storage.max_artifact_age_days,
        },
        agent: AgentConfigInfo {
            adapter: global_config.agent.adapter.clone(),
            model: global_config.agent.options.model.clone(),
            max_turns: global_config.agent.options.max_turns,
        },
    };

    // Load repo configuration if repo_id is provided
    let repo_info = if let Some(repo_id) = params.repo_id {
        let db = ctx.db.lock().await;
        match db.get_repo(&repo_id) {
            Ok(Some(repo)) => {
                let workflows_dir = repo.working_path.join(REPO_CONFIG_PATH);
                let config_exists = workflows_dir.is_dir();

                // Load workflow files from .airlock/workflows/
                let workflows: Vec<WorkflowFileInfo> = if config_exists {
                    match load_workflows_from_disk(&repo.working_path) {
                        Ok(wfs) => wfs
                            .into_iter()
                            .map(|(filename, wf)| WorkflowFileInfo {
                                filename,
                                name: wf.name,
                            })
                            .collect(),
                        Err(e) => {
                            warn!("Failed to load workflows: {}", e);
                            vec![]
                        }
                    }
                } else {
                    vec![]
                };

                Some(RepoConfigInfo {
                    repo_id,
                    working_path: repo.working_path.to_string_lossy().to_string(),
                    config_exists,
                    config_path: workflows_dir.to_string_lossy().to_string(),
                    workflows,
                })
            }
            Ok(None) => {
                debug!("Repo not found for config lookup: {}", repo_id);
                None
            }
            Err(e) => {
                warn!("Database error looking up repo: {}", e);
                None
            }
        }
    } else {
        None
    };

    let result = GetConfigResult {
        global: global_info,
        repo: repo_info,
    };

    Response::success(id, serde_json::to_value(result).unwrap())
}

/// Handle the `update_config` method.
///
/// Updates global configuration.
/// Repo workflow configuration is managed via `.airlock/workflows/` files directly.
pub async fn handle_update_config(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    let params: UpdateConfigParams = match parse_params(params, &id) {
        Ok(p) => p,
        Err(r) => return r,
    };

    let mut global_updated = false;
    let mut global_config_path: Option<String> = None;

    // Update global configuration if provided
    if let Some(global_update) = params.global {
        let config_path = ctx.paths.global_config();
        global_config_path = Some(config_path.to_string_lossy().to_string());

        // Load existing config or use defaults
        let mut global_config = if config_path.exists() {
            match load_global_config(&config_path) {
                Ok(config) => config,
                Err(e) => {
                    warn!(
                        "Failed to load existing global config: {}, starting fresh",
                        e
                    );
                    airlock_core::GlobalConfig::default()
                }
            }
        } else {
            airlock_core::GlobalConfig::default()
        };

        // Apply sync updates
        if let Some(sync_update) = global_update.sync {
            if let Some(on_fetch) = sync_update.on_fetch {
                global_config.sync.on_fetch = on_fetch;
            }
        }

        // Apply storage updates
        if let Some(storage_update) = global_update.storage {
            if let Some(max_artifact_age_days) = storage_update.max_artifact_age_days {
                global_config.storage.max_artifact_age_days = max_artifact_age_days;
            }
        }

        // Ensure parent directory exists
        if let Some(parent) = config_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Response::error(
                    id,
                    error_codes::INTERNAL_ERROR,
                    format!("Failed to create config directory: {}", e),
                );
            }
        }

        // Write config file
        let yaml = match serde_yaml::to_string(&global_config) {
            Ok(y) => y,
            Err(e) => {
                return Response::error(
                    id,
                    error_codes::INTERNAL_ERROR,
                    format!("Failed to serialize config: {}", e),
                )
            }
        };

        if let Err(e) = std::fs::write(&config_path, yaml) {
            return Response::error(
                id,
                error_codes::INTERNAL_ERROR,
                format!("Failed to write config file: {}", e),
            );
        }

        global_updated = true;
        info!("Updated global config at {:?}", config_path);
    }

    let result = UpdateConfigResult {
        success: global_updated,
        global_updated,
        repo_updated: false,
        global_config_path,
        repo_config_path: None,
    };

    Response::success(id, serde_json::to_value(result).unwrap())
}
