//! Status handlers.
//!
//! Handles status, health, and repo listing requests.

use super::util::parse_params;
use super::HandlerContext;
use crate::ipc::{
    error_codes, GetReposResult, HealthResult, RepoInfo, RepoWithStatus, Response, RunInfo,
    StatusParams, StatusResult, SyncInfo,
};
use std::sync::Arc;

/// Handle the `status` method.
pub async fn handle_status(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    let params: StatusParams = match parse_params(params, &id) {
        Ok(p) => p,
        Err(r) => return r,
    };

    let db = ctx.db.lock().await;

    // Get repo
    let repo = match db.get_repo(&params.repo_id) {
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
                format!("Failed to query database: {e}"),
            )
        }
    };

    // Get runs
    let runs = match db.list_runs(&repo.id, Some(10)) {
        Ok(r) => r,
        Err(e) => {
            return Response::error(
                id,
                error_codes::DATABASE_ERROR,
                format!("Failed to list runs: {e}"),
            )
        }
    };

    // Count pending runs using derived status
    let mut pending_runs = 0u32;
    for r in &runs {
        let stages = db.get_step_results_for_run(&r.id).unwrap_or_default();
        let status = r.derived_status(&stages);
        if status == "running" || status == "awaiting_approval" || status == "pending" {
            pending_runs += 1;
        }
    }

    // Get latest run with derived status
    let latest_run = runs.first().map(|r| {
        let stages = db.get_step_results_for_run(&r.id).unwrap_or_default();
        let status = r.derived_status(&stages).to_string();
        let completed_at = if status == "completed" || status == "failed" {
            stages.iter().filter_map(|s| s.completed_at).max()
        } else {
            None
        };
        RunInfo {
            id: r.id.clone(),
            repo_id: None,
            status,
            branch: if r.branch.is_empty() {
                None
            } else {
                Some(r.branch.clone())
            },
            base_sha: None,
            head_sha: None,
            current_step: r.current_step.clone(),
            created_at: r.created_at,
            updated_at: Some(r.updated_at),
            completed_at,
            error: r.error.clone(),
        }
    });

    // Get last sync
    let last_sync = match db.get_latest_sync_log(&repo.id) {
        Ok(Some(log)) => Some(SyncInfo {
            success: log.success,
            synced_at: log.synced_at,
            error: log.error,
        }),
        _ => None,
    };

    let result = StatusResult {
        repo: RepoInfo {
            id: repo.id,
            working_path: repo.working_path.to_string_lossy().to_string(),
            upstream_url: repo.upstream_url,
            gate_path: repo.gate_path.to_string_lossy().to_string(),
            created_at: repo.created_at,
        },
        pending_runs,
        latest_run,
        last_sync,
    };

    Response::success(id, serde_json::to_value(result).unwrap())
}

/// Handle the `health` method.
pub async fn handle_health(ctx: Arc<HandlerContext>, id: serde_json::Value) -> Response {
    let db = ctx.db.lock().await;

    // Check database
    let (database_ok, repo_count) = match db.list_repos() {
        Ok(repos) => (true, repos.len() as u32),
        Err(_) => (false, 0),
    };

    let result = HealthResult {
        healthy: database_ok,
        version: env!("CARGO_PKG_VERSION").to_string(),
        repo_count,
        database_ok,
        socket_path: ctx.paths.socket().to_string_lossy().to_string(),
    };

    Response::success(id, serde_json::to_value(result).unwrap())
}

/// Handle the `get_repos` method.
///
/// Returns a list of all enrolled repositories with their current status,
/// including pending run counts and gate health.
pub async fn handle_get_repos(ctx: Arc<HandlerContext>, id: serde_json::Value) -> Response {
    let db = ctx.db.lock().await;

    // Get all repos
    let repos = match db.list_repos() {
        Ok(r) => r,
        Err(e) => {
            return Response::error(
                id,
                error_codes::DATABASE_ERROR,
                format!("Failed to list repos: {e}"),
            )
        }
    };

    // Build repo list with status information
    let mut repos_with_status = Vec::with_capacity(repos.len());

    for repo in repos {
        // Count pending runs for this repo using derived status
        let pending_runs = match db.list_runs(&repo.id, Some(100)) {
            Ok(runs) => {
                let mut count = 0u32;
                for r in &runs {
                    let stages = db.get_step_results_for_run(&r.id).unwrap_or_default();
                    let status = r.derived_status(&stages);
                    if status == "running" || status == "awaiting_approval" || status == "pending" {
                        count += 1;
                    }
                }
                count
            }
            Err(_) => 0,
        };

        // Check if gate path exists and is a valid git repo
        let gate_healthy = repo.gate_path.exists() && repo.gate_path.join("HEAD").exists();

        repos_with_status.push(RepoWithStatus {
            id: repo.id,
            working_path: repo.working_path.to_string_lossy().to_string(),
            upstream_url: repo.upstream_url,
            gate_path: repo.gate_path.to_string_lossy().to_string(),
            created_at: repo.created_at,
            pending_runs,
            last_sync: repo.last_sync,
            gate_healthy,
        });
    }

    let result = GetReposResult {
        repos: repos_with_status,
    };

    Response::success(id, serde_json::to_value(result).unwrap())
}
