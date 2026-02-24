//! Run handlers.
//!
//! Handles run listing, detail retrieval, and reprocessing.

use super::pipeline::execute_pipeline;
use super::util::{load_artifacts, parse_params};
use super::HandlerContext;
use crate::ipc::{
    error_codes, GetRunDetailParams, GetRunDetailResult, GetRunsParams, GetRunsResult,
    JobResultInfo, RefUpdateParam, ReprocessRunParams, ReprocessRunResult, Response, RunDetailInfo,
    RunInfo, StepResultInfo,
};
use airlock_core::StepStatus;
use std::sync::Arc;
use tracing::{info, warn};

/// Handle the `get_runs` method.
pub async fn handle_get_runs(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    let params: GetRunsParams = match parse_params(params, &id) {
        Ok(p) => p,
        Err(r) => return r,
    };

    let db = ctx.db.lock().await;

    // Verify repo exists
    match db.get_repo(&params.repo_id) {
        Ok(Some(_)) => {}
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

    // Get runs
    let runs = match db.list_runs(&params.repo_id, params.limit) {
        Ok(r) => r,
        Err(e) => {
            return Response::error(
                id,
                error_codes::DATABASE_ERROR,
                format!("Failed to list runs: {}", e),
            )
        }
    };

    // For each run, get job results to compute derived status and completed_at
    let mut run_infos = Vec::with_capacity(runs.len());
    for r in runs {
        let job_results = db.get_job_results_for_run(&r.id).unwrap_or_default();
        let status = r.derived_status_from_jobs(&job_results).to_string();
        // Compute completed_at from max job completed_at when run is done
        let completed_at = if status == "completed" || status == "failed" || status == "superseded"
        {
            job_results.iter().filter_map(|j| j.completed_at).max()
        } else {
            None
        };
        run_infos.push(RunInfo {
            id: r.id,
            repo_id: Some(params.repo_id.clone()),
            status,
            branch: if r.branch.is_empty() {
                None
            } else {
                Some(r.branch)
            },
            base_sha: None,
            head_sha: None,
            current_step: r.current_step,
            created_at: r.created_at,
            updated_at: Some(r.updated_at),
            completed_at,
            error: r.error,
        });
    }

    let result = GetRunsResult { runs: run_infos };

    Response::success(id, serde_json::to_value(result).unwrap())
}

/// Handle the `get_run_detail` method.
pub async fn handle_get_run_detail(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    let params: GetRunDetailParams = match parse_params(params, &id) {
        Ok(p) => p,
        Err(r) => return r,
    };

    let db = ctx.db.lock().await;

    // Get run
    let run = match db.get_run(&params.run_id) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return Response::error(
                id,
                error_codes::RUN_NOT_FOUND,
                format!("Run not found: {}", params.run_id),
            )
        }
        Err(e) => {
            return Response::error(
                id,
                error_codes::DATABASE_ERROR,
                format!("Failed to query database: {}", e),
            )
        }
    };

    // Get job results from database
    let db_job_results = match db.get_job_results_for_run(&run.id) {
        Ok(r) => r,
        Err(e) => {
            warn!("Failed to get job results: {}", e);
            vec![]
        }
    };

    // Get step results from database
    let db_step_results = match db.get_step_results_for_run(&run.id) {
        Ok(r) => r,
        Err(e) => {
            warn!("Failed to get step results: {}", e);
            vec![]
        }
    };

    // Compute derived status from job results
    let status = run.derived_status_from_jobs(&db_job_results).to_string();
    let completed_at = if status == "completed" || status == "failed" || status == "superseded" {
        db_job_results.iter().filter_map(|j| j.completed_at).max()
    } else {
        None
    };

    // Build a job_key lookup from job results
    let job_key_map: std::collections::HashMap<String, String> = db_job_results
        .iter()
        .map(|j| (j.id.clone(), j.job_key.clone()))
        .collect();

    // Convert database job results to IPC format
    let jobs: Vec<JobResultInfo> = db_job_results
        .iter()
        .map(|jr| JobResultInfo {
            id: jr.id.clone(),
            job_key: jr.job_key.clone(),
            name: jr.name.clone(),
            status: airlock_core::job_status_to_string(jr.status).to_string(),
            job_order: jr.job_order,
            started_at: jr.started_at,
            completed_at: jr.completed_at,
            error: jr.error.clone(),
        })
        .collect();

    // Convert database step results to IPC format
    let step_results: Vec<StepResultInfo> = db_step_results
        .iter()
        .map(|sr| StepResultInfo {
            id: sr.id.clone(),
            job_id: sr.job_id.clone(),
            job_key: job_key_map.get(&sr.job_id).cloned().unwrap_or_default(),
            step: sr.name.clone(),
            status: step_status_to_string(sr.status).to_string(),
            exit_code: sr.exit_code,
            duration_ms: sr.duration_ms.map(|d| d as u64),
            error: sr.error.clone(),
            started_at: sr.started_at,
            completed_at: sr.completed_at,
        })
        .collect();

    // Load artifacts from filesystem
    let artifacts = load_artifacts(&ctx.paths, &run.repo_id, &run.id);

    let result = GetRunDetailResult {
        run: RunDetailInfo {
            id: run.id.clone(),
            repo_id: run.repo_id.clone(),
            status,
            branch: run.branch.clone(),
            base_sha: run.base_sha.clone(),
            head_sha: run.head_sha.clone(),
            current_step: run.current_step.clone(),
            workflow_file: run.workflow_file.clone(),
            workflow_name: run.workflow_name.clone(),
            ref_updates: run
                .ref_updates
                .into_iter()
                .map(|r| RefUpdateParam {
                    ref_name: r.ref_name,
                    old_sha: r.old_sha,
                    new_sha: r.new_sha,
                })
                .collect(),
            error: run.error,
            created_at: run.created_at,
            updated_at: run.updated_at,
            completed_at,
        },
        jobs,
        step_results,
        artifacts,
    };

    Response::success(id, serde_json::to_value(result).unwrap())
}

/// Handle the `reprocess_run` method.
///
/// Re-runs the full pipeline for a run. This:
/// 1. Validates the run exists and is in a reprocessable state
/// 2. Clears existing results for the run
/// 3. Resets the run status to Running
/// 4. Re-executes the pipeline
///
/// Needed for Section 4.6: "Reprocess button re-runs pipeline"
pub async fn handle_reprocess_run(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    let params: ReprocessRunParams = match parse_params(params, &id) {
        Ok(p) => p,
        Err(r) => return r,
    };

    // Get run
    let mut run = {
        let db = ctx.db.lock().await;
        match db.get_run(&params.run_id) {
            Ok(Some(r)) => r,
            Ok(None) => {
                return Response::error(
                    id,
                    error_codes::RUN_NOT_FOUND,
                    format!("Run not found: {}", params.run_id),
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

    // Check if run is in a reprocessable state
    // Running runs should not be reprocessed (would cause conflicts)
    let jobs = {
        let db = ctx.db.lock().await;
        db.get_job_results_for_run(&run.id).unwrap_or_default()
    };
    if run.is_running_from_jobs(&jobs) {
        return Response::error(
            id,
            error_codes::INVALID_REPO_STATE,
            "Cannot reprocess a run that is still running".to_string(),
        );
    }

    // Get repo
    let repo = {
        let db = ctx.db.lock().await;
        match db.get_repo(&run.repo_id) {
            Ok(Some(r)) => r,
            Ok(None) => {
                return Response::error(
                    id,
                    error_codes::REPO_NOT_FOUND,
                    format!("Repository not found: {}", run.repo_id),
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

    // Delete existing job and step results for this run
    {
        let db = ctx.db.lock().await;
        if let Err(e) = db.delete_job_results_for_run(&params.run_id) {
            warn!("Failed to delete existing job results: {}", e);
        }
        if let Err(e) = db.delete_step_results_for_run(&params.run_id) {
            warn!("Failed to delete existing step results: {}", e);
            // Continue anyway - old results won't break anything
        }
    }

    // Clear any previous error on the run
    run.error = None;

    {
        let db = ctx.db.lock().await;
        if let Err(e) = db.update_run_error(&params.run_id, None) {
            return Response::error(
                id,
                error_codes::DATABASE_ERROR,
                format!("Failed to clear run error: {}", e),
            );
        }
    }

    info!("Reprocessing run {} for repo {}", run.id, repo.id);

    // Spawn the pipeline through the run queue so we can return immediately.
    // The frontend tracks progress via RunUpdated / RunCompleted events.
    tokio::spawn(async move {
        let permit = ctx.run_queue.acquire(&run.repo_id).await;
        execute_pipeline(ctx.clone(), run, repo, permit.token).await;
    });

    let result = ReprocessRunResult {
        run_id: params.run_id,
        success: true,
        new_status: "running".to_string(),
    };

    Response::success(id, serde_json::to_value(result).unwrap())
}

/// Convert StepStatus to string for IPC responses.
fn step_status_to_string(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Pending => "pending",
        StepStatus::Running => "running",
        StepStatus::Passed => "passed",
        StepStatus::Failed => "failed",
        StepStatus::Skipped => "skipped",
        StepStatus::AwaitingApproval => "awaiting_approval",
    }
}
