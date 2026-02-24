//! Sync handlers.
//!
//! Handles repository synchronization with upstream.

use super::util::{now, parse_params};
use super::HandlerContext;
use crate::ipc::{
    error_codes, FetchNotificationParams, FetchNotificationResult, Response, SyncAllResult,
    SyncError, SyncParams, SyncResult,
};
use crate::sync;
use airlock_core::{git, SyncLog};
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Handle the `sync` method.
pub async fn handle_sync(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    let params: SyncParams = match parse_params(params, &id) {
        Ok(p) => p,
        Err(r) => return r,
    };

    // Look up repo
    let repo = {
        let db = ctx.db.lock().await;
        match db.get_repo(&params.repo_id) {
            Ok(Some(r)) => r,
            Ok(None) => {
                return Response::error(
                    id,
                    error_codes::REPO_NOT_FOUND,
                    format!("Repository not found: {}", params.repo_id),
                )
            }
            Err(e) => {
                return Response::error(
                    id,
                    error_codes::DATABASE_ERROR,
                    format!("Failed to query database: {}", e),
                )
            }
        }
    };

    // Smart sync from upstream, preserving un-forwarded commits
    let synced_at = now();
    let sync_worktree_dir = ctx.paths.sync_worktree_dir(&repo.id);
    let (success, error) = match git::smart_sync_from_remote(
        &repo.gate_path,
        "origin",
        Some(&sync_worktree_dir),
        git::ConflictResolver::Agent,
    ) {
        Ok(report) => {
            if !report.warnings.is_empty() {
                for warning in &report.warnings {
                    warn!("Sync warning for repo {}: {}", repo.id, warning);
                }
            }
            (true, None)
        }
        Err(e) => (false, Some(e.to_string())),
    };

    // Record sync log
    let sync_log = SyncLog {
        id: uuid::Uuid::new_v4().to_string(),
        repo_id: repo.id.clone(),
        success,
        error: error.clone(),
        synced_at,
    };

    {
        let db = ctx.db.lock().await;
        if let Err(e) = db.insert_sync_log(&sync_log) {
            warn!("Failed to record sync log: {}", e);
        }
        if success {
            if let Err(e) = db.update_repo_last_sync(&repo.id, synced_at) {
                warn!("Failed to update last sync: {}", e);
            }
        }
    }

    let result = SyncResult {
        success,
        error,
        synced_at,
    };

    Response::success(id, serde_json::to_value(result).unwrap())
}

/// Handle the `sync_all` method.
pub async fn handle_sync_all(ctx: Arc<HandlerContext>, id: serde_json::Value) -> Response {
    // Get all repos
    let repos = {
        let db = ctx.db.lock().await;
        match db.list_repos() {
            Ok(r) => r,
            Err(e) => {
                return Response::error(
                    id,
                    error_codes::DATABASE_ERROR,
                    format!("Failed to list repos: {}", e),
                )
            }
        }
    };

    let mut synced_count = 0u32;
    let mut failed_count = 0u32;
    let mut errors = Vec::new();

    for repo in repos {
        let synced_at = now();
        // Smart sync from upstream, preserving un-forwarded commits
        let sync_worktree_dir = ctx.paths.sync_worktree_dir(&repo.id);
        match git::smart_sync_from_remote(
            &repo.gate_path,
            "origin",
            Some(&sync_worktree_dir),
            git::ConflictResolver::Agent,
        ) {
            Ok(report) => {
                if !report.warnings.is_empty() {
                    for warning in &report.warnings {
                        warn!("Sync warning for repo {}: {}", repo.id, warning);
                    }
                }
                synced_count += 1;
                let db = ctx.db.lock().await;
                let _ = db.update_repo_last_sync(&repo.id, synced_at);

                // Record sync log
                let sync_log = SyncLog {
                    id: uuid::Uuid::new_v4().to_string(),
                    repo_id: repo.id.clone(),
                    success: true,
                    error: None,
                    synced_at,
                };
                let _ = db.insert_sync_log(&sync_log);
            }
            Err(e) => {
                failed_count += 1;
                errors.push(SyncError {
                    repo_id: repo.id.clone(),
                    error: e.to_string(),
                });

                // Record sync log
                let db = ctx.db.lock().await;
                let sync_log = SyncLog {
                    id: uuid::Uuid::new_v4().to_string(),
                    repo_id: repo.id.clone(),
                    success: false,
                    error: Some(e.to_string()),
                    synced_at,
                };
                let _ = db.insert_sync_log(&sync_log);
            }
        }
    }

    let result = SyncAllResult {
        synced_count,
        failed_count,
        errors,
    };

    Response::success(id, serde_json::to_value(result).unwrap())
}

/// Handle the `fetch_notification` method.
///
/// This is called by the upload-pack wrapper when someone fetches from the gate.
/// It triggers the sync-on-fetch logic:
/// 1. Check if the repo is stale (>5 seconds since last sync)
/// 2. If stale, sync from upstream before the fetch completes
pub async fn handle_fetch_notification(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    let params: FetchNotificationParams = match parse_params(params, &id) {
        Ok(p) => p,
        Err(r) => return r,
    };

    debug!("Received fetch notification for gate: {}", params.gate_path);

    // Look up repo by gate path
    let gate_path = Path::new(&params.gate_path);
    let repo = {
        let db = ctx.db.lock().await;
        let repos = match db.list_repos() {
            Ok(r) => r,
            Err(e) => {
                return Response::error(
                    id,
                    error_codes::DATABASE_ERROR,
                    format!("Failed to list repos: {}", e),
                )
            }
        };

        repos.into_iter().find(|r| r.gate_path == gate_path)
    };

    let repo = match repo {
        Some(r) => r,
        None => {
            warn!("No repo found for gate path: {}", params.gate_path);
            // Return success with empty result - repo not managed by Airlock
            let result = FetchNotificationResult {
                synced: false,
                success: true,
                error: None,
                skipped_not_stale: false,
                repo_id: None,
            };
            return Response::success(id, serde_json::to_value(result).unwrap());
        }
    };

    let repo_id = repo.id.clone();
    debug!("Found repo {} for fetch notification", repo_id);

    // Perform sync-on-fetch logic (does not touch database)
    let sync_result = sync::sync_if_stale(&ctx.paths, &repo).await;

    // Update database if sync was successful
    if sync_result.synced && sync_result.success {
        let db = ctx.db.lock().await;
        // Record sync log
        let sync_log = SyncLog {
            id: uuid::Uuid::new_v4().to_string(),
            repo_id: repo_id.clone(),
            success: true,
            error: None,
            synced_at: sync_result.timestamp,
        };
        if let Err(e) = db.insert_sync_log(&sync_log) {
            warn!("Failed to record sync log: {}", e);
        }
        // Update last_sync
        if let Err(e) = db.update_repo_last_sync(&repo_id, sync_result.timestamp) {
            warn!("Failed to update last_sync: {}", e);
        }
    } else if sync_result.synced && !sync_result.success {
        // Record failed sync
        let db = ctx.db.lock().await;
        let sync_log = SyncLog {
            id: uuid::Uuid::new_v4().to_string(),
            repo_id: repo_id.clone(),
            success: false,
            error: sync_result.error.clone(),
            synced_at: sync_result.timestamp,
        };
        if let Err(e) = db.insert_sync_log(&sync_log) {
            warn!("Failed to record sync log: {}", e);
        }
    }

    let result = FetchNotificationResult {
        synced: sync_result.synced,
        success: sync_result.success,
        error: sync_result.error.clone(),
        skipped_not_stale: sync_result.skipped_not_stale,
        repo_id: Some(repo_id.clone()),
    };

    if sync_result.synced {
        if sync_result.success {
            info!("Sync-on-fetch completed for repo {}", repo_id);
        } else {
            warn!(
                "Sync-on-fetch failed for repo {}: {:?}",
                repo_id, sync_result.error
            );
        }
    } else if sync_result.skipped_not_stale {
        debug!("Sync-on-fetch skipped for repo {} (not stale)", repo_id);
    }

    Response::success(id, serde_json::to_value(result).unwrap())
}
