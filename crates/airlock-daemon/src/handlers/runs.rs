//! Run handlers.
//!
//! Handles run listing, detail retrieval, and reprocessing.

use super::pipeline::{
    emit_run_final_status, execute_pipeline, execute_single_job, extract_branch_name,
    load_workflows_for_run, resolve_job_worktree, resume_dag_after_job_completion,
};
use super::util::{load_artifacts, parse_params};
use super::HandlerContext;
use crate::ipc::{
    error_codes, CancelRunParams, CancelRunResult, GetRunDetailParams, GetRunDetailResult,
    GetRunsParams, GetRunsResult, JobResultInfo, RefUpdateParam, ReprocessRunParams,
    ReprocessRunResult, Response, RetryJobParams, RetryJobResult, RunDetailInfo, RunInfo,
    StepResultInfo,
};
use airlock_core::{step_status_to_string, JobStatus, WorkflowConfig};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tracing::{error, info, warn};

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
                format!("Failed to query database: {e}"),
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
                format!("Failed to list runs: {e}"),
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
                format!("Failed to query database: {e}"),
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
                    format!("Failed to query database: {e}"),
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
                    format!("Failed to query database: {e}"),
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
                format!("Failed to clear run error: {e}"),
            );
        }
    }

    info!("Reprocessing run {} for repo {}", run.id, repo.id);

    // Spawn the pipeline through the run queue so we can return immediately.
    // The frontend tracks progress via RunUpdated / RunCompleted events.
    let ref_names: Vec<String> = run.ref_updates.iter().map(|u| u.ref_name.clone()).collect();
    tokio::spawn(async move {
        let permit = ctx.run_queue.acquire(&run.repo_id, &ref_names).await;
        execute_pipeline(ctx.clone(), run, repo, permit.token.clone()).await;
    });

    let result = ReprocessRunResult {
        run_id: params.run_id,
        success: true,
        new_status: "running".to_string(),
    };

    Response::success(id, serde_json::to_value(result).unwrap())
}

/// Handle the `cancel_run` method.
///
/// Cancels a currently running pipeline run. This:
/// 1. Validates the run exists and is currently running
/// 2. Sets the run error to "Stopped by user" in the database
/// 3. Triggers the CancellationToken via the run queue
pub async fn handle_cancel_run(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    let params: CancelRunParams = match parse_params(params, &id) {
        Ok(p) => p,
        Err(r) => return r,
    };

    // Get run and verify it's running
    let run = {
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
                    format!("Failed to query database: {e}"),
                )
            }
        }
    };

    let jobs = {
        let db = ctx.db.lock().await;
        db.get_job_results_for_run(&run.id).unwrap_or_default()
    };

    if !run.is_running_from_jobs(&jobs) {
        return Response::error(
            id,
            error_codes::INVALID_REPO_STATE,
            "Cannot cancel a run that is not running".to_string(),
        );
    }

    // Set the run error before cancelling so mark_run_cancelled preserves it
    {
        let db = ctx.db.lock().await;
        if let Err(e) = db.update_run_error(&params.run_id, Some("Stopped by user")) {
            warn!("Failed to set run error: {}", e);
        }
    }

    // Cancel the active run via the run queue
    let branch = run.branch.clone();
    let ref_names = if branch.is_empty() {
        None
    } else {
        Some(vec![format!("refs/heads/{}", branch)])
    };
    ctx.run_queue
        .cancel_active(&run.repo_id, ref_names.as_deref());

    info!("Cancelled run {} for repo {}", run.id, run.repo_id);

    let result = CancelRunResult {
        run_id: params.run_id,
        success: true,
    };

    Response::success(id, serde_json::to_value(result).unwrap())
}

/// Collect transitive downstream dependents of a job in a workflow DAG.
///
/// BFS from the target job: for each job in the workflow, if any of its `needs`
/// entries is in the frontier, it is a downstream dependent.
pub fn collect_downstream(target: &str, workflow: &WorkflowConfig) -> Vec<String> {
    let mut downstream = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(target.to_string());

    while let Some(current) = queue.pop_front() {
        for (job_key, job_config) in workflow.jobs.iter() {
            if downstream.contains(job_key.as_str()) {
                continue;
            }
            if job_key == target {
                continue;
            }
            if job_config.needs.iter().any(|dep| dep == &current) {
                downstream.insert(job_key.clone());
                queue.push_back(job_key.clone());
            }
        }
    }

    downstream.into_iter().collect()
}

/// Handle the `retry_job` method.
///
/// Retries a specific failed/skipped job by resetting it and its transitive
/// downstream dependents to Pending, then re-executing from that job.
pub async fn handle_retry_job(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    let params: RetryJobParams = match parse_params(params, &id) {
        Ok(p) => p,
        Err(r) => return r,
    };

    // Phase 1: DB reads + validation (hold lock briefly)
    let (run, repo) = {
        let db = ctx.db.lock().await;

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
                    format!("Failed to query database: {e}"),
                )
            }
        };

        let jobs = match db.get_job_results_for_run(&run.id) {
            Ok(j) => j,
            Err(e) => {
                return Response::error(
                    id,
                    error_codes::DATABASE_ERROR,
                    format!("Failed to get job results: {e}"),
                )
            }
        };
        if run.is_running_from_jobs(&jobs) {
            return Response::error(
                id,
                error_codes::INVALID_REPO_STATE,
                "Cannot retry a job while the run is still running".to_string(),
            );
        }

        let target_job = match jobs.iter().find(|j| j.job_key == params.job_key) {
            Some(j) => j,
            None => {
                return Response::error(
                    id,
                    error_codes::STEP_NOT_FOUND,
                    format!(
                        "Job '{}' not found in run {}",
                        params.job_key, params.run_id
                    ),
                )
            }
        };

        if target_job.status != JobStatus::Failed && target_job.status != JobStatus::Skipped {
            return Response::error(
                id,
                error_codes::JOB_NOT_RETRYABLE,
                format!(
                    "Job '{}' has status '{}' — only failed or skipped jobs can be retried",
                    params.job_key,
                    airlock_core::job_status_to_string(target_job.status)
                ),
            );
        }

        let repo = match db.get_repo(&run.repo_id) {
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
                    format!("Failed to query database: {e}"),
                )
            }
        };

        (run, repo)
    };

    // Phase 2: Load workflows from filesystem (no DB lock held)
    let branch = extract_branch_name(&run.ref_updates);
    let workflows = match load_workflows_for_run(&repo.gate_path, &run.head_sha, branch.as_deref())
    {
        Ok(w) => w,
        Err(e) => {
            return Response::error(
                id,
                error_codes::INTERNAL_ERROR,
                format!("Failed to load workflows: {e}"),
            )
        }
    };

    let workflow = match workflows
        .iter()
        .find(|(_, wf)| wf.jobs.contains_key(&params.job_key))
    {
        Some((_, wf)) => wf.clone(),
        None => {
            return Response::error(
                id,
                error_codes::INTERNAL_ERROR,
                format!(
                    "Job '{}' not found in any workflow for this run",
                    params.job_key
                ),
            )
        }
    };

    let downstream = collect_downstream(&params.job_key, &workflow);
    let mut reset_jobs = vec![params.job_key.clone()];
    reset_jobs.extend(downstream.iter().cloned());

    // Phase 3: Re-check guard + reset under lock (prevents TOCTOU races)
    let jobs = {
        let db = ctx.db.lock().await;

        // Re-check that run hasn't become active since we released the lock
        let fresh_jobs = match db.get_job_results_for_run(&run.id) {
            Ok(j) => j,
            Err(e) => {
                return Response::error(
                    id,
                    error_codes::DATABASE_ERROR,
                    format!("Failed to get job results: {e}"),
                )
            }
        };
        if run.is_running_from_jobs(&fresh_jobs) {
            return Response::error(
                id,
                error_codes::INVALID_REPO_STATE,
                "Cannot retry a job while the run is still running".to_string(),
            );
        }

        // Re-check that the target job is still Failed/Skipped (another retry
        // may have already reset it to Pending between phase 1 and now).
        if let Some(target) = fresh_jobs.iter().find(|j| j.job_key == params.job_key) {
            if target.status != JobStatus::Failed && target.status != JobStatus::Skipped {
                return Response::error(
                    id,
                    error_codes::JOB_NOT_RETRYABLE,
                    format!(
                        "Job '{}' is no longer retryable (status: '{}')",
                        params.job_key,
                        airlock_core::job_status_to_string(target.status)
                    ),
                );
            }
        }

        // Reset target + downstream jobs and their steps.
        // The target job must reset successfully; downstream failures are non-fatal.
        for job_key in &reset_jobs {
            if let Some(job) = fresh_jobs.iter().find(|j| &j.job_key == job_key) {
                let is_target = job_key == &params.job_key;
                if let Err(e) = db.reset_job_to_pending(&job.id) {
                    if is_target {
                        return Response::error(
                            id,
                            error_codes::DATABASE_ERROR,
                            format!("Failed to reset job '{}': {e}", job_key),
                        );
                    }
                    warn!("Failed to reset downstream job '{}': {}", job_key, e);
                }
                if let Err(e) = db.reset_step_results_for_job(&job.id) {
                    if is_target {
                        return Response::error(
                            id,
                            error_codes::DATABASE_ERROR,
                            format!("Failed to reset steps for job '{}': {e}", job_key),
                        );
                    }
                    warn!(
                        "Failed to reset steps for downstream job '{}': {}",
                        job_key, e
                    );
                }
            }
        }

        // Only clear run error if no other jobs are still failed (outside the reset set)
        let other_failures = fresh_jobs
            .iter()
            .any(|j| j.status == JobStatus::Failed && !reset_jobs.contains(&j.job_key));
        if !other_failures {
            if let Err(e) = db.update_run_error(&params.run_id, None) {
                warn!("Failed to clear run error: {}", e);
            }
        }

        fresh_jobs
    };

    info!(
        "Retrying job '{}' in run {} (resetting {} jobs: {:?})",
        params.job_key,
        run.id,
        reset_jobs.len(),
        reset_jobs,
    );

    // Build job_id_map from fresh_jobs (the authoritative state after reset)
    let job_id_map: HashMap<String, String> = jobs
        .iter()
        .map(|j| (j.job_key.clone(), j.id.clone()))
        .collect();
    let job_key = params.job_key.clone();
    let job_config = match workflow.jobs.get(&job_key) {
        Some(c) => c.clone(),
        None => {
            return Response::error(
                id,
                error_codes::INTERNAL_ERROR,
                format!("Job '{}' no longer found in workflow config", job_key),
            )
        }
    };
    let run_clone = run.clone();
    let repo_clone = repo.clone();

    // Spawn background task to execute the retried job.
    // RunUpdated is emitted after acquiring the permit so the UI doesn't
    // show "running" while the job is still queued.
    let downstream_keys: Vec<String> = reset_jobs[1..].to_vec();
    tokio::spawn(async move {
        let ref_names: Vec<String> = run_clone
            .ref_updates
            .iter()
            .map(|u| u.ref_name.clone())
            .collect();
        let permit = ctx.run_queue.acquire(&run_clone.repo_id, &ref_names).await;

        ctx.emit(crate::ipc::AirlockEvent::RunUpdated {
            repo_id: run_clone.repo_id.clone(),
            run_id: run_clone.id.clone(),
            status: "running".to_string(),
        });

        // Resolve worktree and execute
        let (worktree_path, _lease) = match resolve_job_worktree(
            &ctx,
            &job_key,
            &run_clone,
            &repo_clone,
            &job_id_map,
        )
        .await
        {
            Ok(result) => result,
            Err(e) => {
                error!(
                    "Failed to resolve worktree for retry of '{}': {}",
                    job_key, e
                );
                // Mark target job as failed
                let db = ctx.db.lock().await;
                if let Some(job_id) = job_id_map.get(&job_key) {
                    let _ = db.update_job_status(
                        job_id,
                        JobStatus::Failed,
                        None,
                        None,
                        Some(&format!("Worktree setup failed: {e}")),
                    );
                }
                // Mark downstream jobs as skipped so the run reaches a final state
                for ds_key in &downstream_keys {
                    if let Some(job_id) = job_id_map.get(ds_key.as_str()) {
                        let _ = db.update_job_status(job_id, JobStatus::Skipped, None, None, None);
                    }
                }
                drop(db);
                emit_run_final_status(&ctx, &run_clone).await;
                drop(permit);
                return;
            }
        };

        // Use the permit's cancellation token so a newer push can cancel this retry
        let status = execute_single_job(
            &ctx,
            &run_clone,
            &repo_clone,
            &job_key,
            &job_config,
            &job_id_map,
            &worktree_path,
            &permit.token,
        )
        .await;

        // Let the DAG machinery handle downstream jobs
        resume_dag_after_job_completion(&ctx, &run_clone, &repo_clone, &workflow, &job_key, status)
            .await;
        emit_run_final_status(&ctx, &run_clone).await;
        drop(permit);
    });

    let result = RetryJobResult {
        run_id: params.run_id,
        job_key: params.job_key,
        success: true,
        reset_jobs,
    };

    Response::success(id, serde_json::to_value(result).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use airlock_core::WorkflowConfig;

    fn make_workflow(yaml: &str) -> WorkflowConfig {
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn test_collect_downstream_linear() {
        // A -> B -> C
        let wf = make_workflow(
            "jobs:
  a:
    steps: []
  b:
    needs: a
    steps: []
  c:
    needs: b
    steps: []",
        );
        let mut result = collect_downstream("a", &wf);
        result.sort();
        assert_eq!(result, vec!["b", "c"]);
    }

    #[test]
    fn test_collect_downstream_diamond() {
        // A -> {B, C} -> D
        let wf = make_workflow(
            "jobs:
  a:
    steps: []
  b:
    needs: a
    steps: []
  c:
    needs: a
    steps: []
  d:
    needs: [b, c]
    steps: []",
        );

        let mut result = collect_downstream("a", &wf);
        result.sort();
        assert_eq!(result, vec!["b", "c", "d"]);

        // Retrying B only collects D
        let result = collect_downstream("b", &wf);
        assert_eq!(result, vec!["d"]);
    }

    #[test]
    fn test_collect_downstream_no_deps() {
        // A -> B, C (leaf)
        let wf = make_workflow(
            "jobs:
  a:
    steps: []
  b:
    needs: a
    steps: []
  c:
    steps: []",
        );
        let result = collect_downstream("c", &wf);
        assert!(result.is_empty());
    }
}
