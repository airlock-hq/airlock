//! Step handlers.
//!
//! Handles step-based pipeline operations: approve steps and get run diffs.

use super::pipeline::{
    extract_branch_name, load_workflows_for_run, now_epoch, resume_dag_after_job_completion,
    step_status_str,
};
use super::util::parse_params;
use super::HandlerContext;
use crate::ipc::{
    error_codes, AirlockEvent, ApplyPatchesParams, ApplyPatchesResult, ApproveStepParams,
    ApproveStepResult, CommitDiffInfo, GetRunDiffParams, GetRunDiffResult, PatchError, Response,
};
use crate::pipeline::LogStreamCallback;
use crate::stage_loader::StageLoader;
use airlock_core::git::compute_diff_with_commits;
use airlock_core::{JobStatus, StepStatus};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Handle the `approve_step` method.
///
/// Approves a step that is awaiting approval. This:
/// 1. Validates the step exists and is in AwaitingApproval status
/// 2. Marks the step as Passed
/// 3. Updates the job status to Running
/// 4. Resumes pipeline execution in the background
pub async fn handle_approve_step(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    let params: ApproveStepParams = match parse_params(params, &id) {
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

    // Verify repo exists
    match db.get_repo(&run.repo_id) {
        Ok(Some(_)) => {}
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
    };

    // Get step results for the run
    let step_results = match db.get_step_results_for_run(&params.run_id) {
        Ok(r) => r,
        Err(e) => {
            return Response::error(
                id,
                error_codes::DATABASE_ERROR,
                format!("Failed to get step results: {}", e),
            )
        }
    };

    // Find the step to approve (matching both step_name and job_key via job_id)
    let step_result = match step_results.iter().find(|r| r.name == params.step_name) {
        Some(sr) => sr.clone(),
        None => {
            return Response::error(
                id,
                error_codes::STEP_NOT_FOUND,
                format!("Step '{}' not found in run", params.step_name),
            )
        }
    };

    // Validate status
    if step_result.status != StepStatus::AwaitingApproval {
        return Response::error(
            id,
            error_codes::INVALID_STEP_STATUS,
            format!(
                "Step '{}' is not awaiting approval (status: {:?})",
                params.step_name, step_result.status
            ),
        );
    }

    // Update step status to Passed
    let mut updated_step = step_result;
    updated_step.status = StepStatus::Passed;
    updated_step.completed_at = Some(now_epoch());

    if let Err(e) = db.update_step_result(&updated_step) {
        return Response::error(
            id,
            error_codes::DATABASE_ERROR,
            format!("Failed to update step result: {}", e),
        );
    }

    // Update the job status from AwaitingApproval back to Running
    if let Ok(jobs) = db.get_job_results_for_run(&params.run_id) {
        if let Some(job) = jobs.iter().find(|j| j.job_key == params.job_key) {
            let _ = db.update_job_status(&job.id, JobStatus::Running, None, None, None);
        }
    }

    info!(
        "Approved step '{}' in job '{}' for run {} (marked as passed, job set to running)",
        params.step_name, params.job_key, params.run_id
    );

    // Get repo for pipeline execution
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
                format!("Failed to query database: {}", e),
            )
        }
    };

    // Release the database lock before spawning async pipeline execution
    drop(db);

    // Spawn background task to resume pipeline execution
    let run_id = params.run_id.clone();
    let step_name = params.step_name.clone();
    let job_key = params.job_key.clone();
    tokio::spawn(async move {
        resume_pipeline_after_approval(ctx, run, repo, &job_key, &step_name).await;
    });

    // Return success immediately - pipeline continues in background
    let response = ApproveStepResult {
        run_id,
        job_key: params.job_key,
        step_name: params.step_name,
        success: true,
        new_step_status: "passed".to_string(),
        pipeline_completed: false, // Will be updated by events as pipeline progresses
        paused_at_step: None,
    };

    Response::success(id, serde_json::to_value(response).unwrap())
}

/// Resume pipeline execution after step approval.
///
/// This function:
/// 1. Resumes execution of the specific job from the step after the approved one
/// 2. When the job completes, checks if dependent jobs can now start
/// 3. Executes newly unblocked dependent jobs (via DAG continuation)
/// 4. Continues until all reachable jobs complete or pause
async fn resume_pipeline_after_approval(
    ctx: Arc<HandlerContext>,
    run: airlock_core::Run,
    repo: airlock_core::Repo,
    approved_job_key: &str,
    approved_step_name: &str,
) {
    info!(
        "Resuming pipeline for run {} after approval of step '{}' in job '{}'",
        run.id, approved_step_name, approved_job_key
    );

    // Load workflow configuration from the pushed commit in the gate
    let branch = extract_branch_name(&run.ref_updates);
    let workflows = match load_workflows_for_run(&repo.gate_path, &run.head_sha, branch.as_deref())
    {
        Ok(w) => w,
        Err(e) => {
            error!("Failed to load workflows for resume: {}", e);
            let db = ctx.db.lock().await;
            let _ = db.update_run_error(&run.id, Some(&e));
            return;
        }
    };

    // Find the workflow containing the approved job
    let workflow = match workflows
        .iter()
        .find(|(_, wf)| wf.jobs.contains_key(approved_job_key))
    {
        Some((_, wf)) => wf.clone(),
        None => {
            error!(
                "Job '{}' not found in any workflow config",
                approved_job_key
            );
            return;
        }
    };

    let job_config = workflow.jobs.get(approved_job_key).unwrap().clone();
    let keep_worktrees = job_config.keep_worktrees;

    // Find the approved step index within the job
    let approved_idx = match job_config
        .steps
        .iter()
        .position(|s| s.name == approved_step_name)
    {
        Some(idx) => idx,
        None => {
            error!(
                "Step '{}' not found in job '{}'",
                approved_step_name, approved_job_key
            );
            return;
        }
    };

    // Get remaining steps after the approved one
    let remaining_steps = &job_config.steps[(approved_idx + 1)..];

    // Handle case where approved step was the last in the job
    if remaining_steps.is_empty() {
        info!(
            "No remaining steps after '{}', job '{}' complete",
            approved_step_name, approved_job_key
        );

        let job_status = JobStatus::Passed;
        finalize_job(&ctx, &run, approved_job_key, job_status, None).await;

        // Emit job completed event
        ctx.emit(AirlockEvent::JobCompleted {
            repo_id: run.repo_id.clone(),
            run_id: run.id.clone(),
            job_key: approved_job_key.to_string(),
            status: "passed".to_string(),
        });

        // Resume DAG: check if dependent jobs can now start
        resume_dag_after_job_completion(&ctx, &run, &repo, &workflow, approved_job_key, job_status)
            .await;

        // Clean up ephemeral worktrees only (persistent worktree is kept)
        if !keep_worktrees {
            let worktree_path = ctx.paths.run_worktree(&run.repo_id, &run.id);
            let persistent_wt = ctx.paths.repo_worktree(&run.repo_id);
            if worktree_path != persistent_wt {
                if let Err(e) = airlock_core::remove_worktree(&repo.gate_path, &worktree_path) {
                    warn!("Failed to remove worktree: {}", e);
                }
            }
        }

        // Emit final run-level events
        emit_run_final_status(&ctx, &run).await;
        return;
    }

    // Get worktree path (should still exist since we preserved it when paused)
    let worktree_path = find_job_worktree(&ctx.paths, &run, approved_job_key);
    if !worktree_path.exists() {
        error!(
            "Worktree at {:?} no longer exists, cannot resume pipeline",
            worktree_path
        );
        let db = ctx.db.lock().await;
        let _ = db.update_run_error(
            &run.id,
            Some("Worktree no longer exists, cannot resume pipeline"),
        );
        return;
    }

    // Get existing step results for this job
    let job_id = {
        let db = ctx.db.lock().await;
        match db.get_job_results_for_run(&run.id) {
            Ok(jobs) => jobs
                .iter()
                .find(|j| j.job_key == approved_job_key)
                .map(|j| j.id.clone()),
            Err(_) => None,
        }
    };

    let job_id = match job_id {
        Some(id) => id,
        None => {
            error!(
                "Job result for '{}' not found in database",
                approved_job_key
            );
            return;
        }
    };

    let mut step_results = {
        let db = ctx.db.lock().await;
        match db.get_step_results_for_job(&job_id) {
            Ok(r) => r,
            Err(e) => {
                error!("Failed to get step results: {}", e);
                return;
            }
        }
    };

    // Create stage loader for resolving reusable stages
    let stage_loader = StageLoader::default();

    // Execute remaining steps
    let mut job_success = true;
    let mut job_error: Option<String> = None;
    let mut paused_for_approval = false;

    for step in remaining_steps {
        // Find the step result for this step
        let step_result = match step_results.iter_mut().find(|r| r.name == step.name) {
            Some(r) => r,
            None => {
                error!("Step result for '{}' not found", step.name);
                continue;
            }
        };

        // Update current step in run
        {
            let db = ctx.db.lock().await;
            if let Err(e) = db.update_run_current_step(&run.id, Some(&step.name)) {
                warn!("Failed to update current step: {}", e);
            }
        }

        info!("Executing step '{}' (resumed after approval)", step.name);

        // Resolve reusable stage if using `uses:` syntax
        let resolved_step = if step.is_reusable() {
            debug!("Resolving reusable action: {:?}", step.uses);
            match stage_loader.resolve_stage(step).await {
                Ok(resolved) => resolved,
                Err(e) => {
                    error!(
                        "Failed to resolve reusable action '{}' (uses: {:?}): {}",
                        step.name, step.uses, e
                    );
                    job_success = false;
                    job_error = Some(format!(
                        "Failed to resolve reusable action '{}': {}",
                        step.name, e
                    ));
                    break;
                }
            }
        } else {
            step.clone()
        };

        // Build environment for this step
        let env_params = crate::pipeline::StageEnvironmentParams {
            paths: &ctx.paths,
            repo_id: &run.repo_id,
            run_id: &run.id,
            stage_name: &resolved_step.name,
            branch: &run.branch,
            base_sha: &run.base_sha,
            head_sha: &run.head_sha,
            worktree_path: &worktree_path,
            repo_root: &repo.working_path,
            upstream_url: &repo.upstream_url,
            gate_path: &repo.gate_path,
            job_key: Some(approved_job_key),
        };

        let env = match crate::pipeline::build_stage_environment(&env_params) {
            Ok(mut e) => {
                e.job_name = job_config.name.clone();
                e
            }
            Err(e) => {
                error!(
                    "Failed to build environment for step '{}': {}",
                    resolved_step.name, e
                );
                job_success = false;
                job_error = Some(format!("Failed to build environment: {}", e));
                break;
            }
        };

        // Mark step as running
        {
            let db = ctx.db.lock().await;
            step_result.status = StepStatus::Running;
            step_result.started_at = Some(now_epoch());
            let _ = db.update_step_result(step_result);
        }

        // Emit StepStarted event
        ctx.emit(AirlockEvent::StepStarted {
            repo_id: run.repo_id.clone(),
            run_id: run.id.clone(),
            job_key: approved_job_key.to_string(),
            step_name: step.name.clone(),
        });

        // Create log streaming callback that both emits events and writes to disk
        let log_repo_id = run.repo_id.clone();
        let log_run_id = run.id.clone();
        let log_job_key = approved_job_key.to_string();
        let log_step_name = step.name.clone();
        let log_ctx = ctx.clone();
        let log_logs_dir = env.logs_dir.clone();
        let log_callback: LogStreamCallback =
            Arc::new(move |stream_type: &str, content: String| {
                // Emit event for real-time UI streaming
                log_ctx.emit(AirlockEvent::LogChunk {
                    repo_id: log_repo_id.clone(),
                    run_id: log_run_id.clone(),
                    job_key: log_job_key.clone(),
                    step_name: log_step_name.clone(),
                    stream: stream_type.to_string(),
                    content: content.clone(),
                });

                // Append to log file on disk so late subscribers can read existing output
                let filename = if stream_type == "stdout" {
                    "stdout.log"
                } else {
                    "stderr.log"
                };
                let path = log_logs_dir.join(filename);
                crate::pipeline::append_log_capped(&path, content.as_bytes());
            });

        // Execute the step
        let timeout = std::time::Duration::from_secs(resolved_step.timeout.unwrap_or(60 * 60));
        let result = crate::pipeline::execute_stage_with_log_callback(
            &resolved_step,
            &step_result.id,
            &run.id,
            &env,
            timeout,
            Some(log_callback),
            None,
        )
        .await;

        match result {
            Ok(res) => {
                *step_result = res.clone();

                // Update step result in database
                {
                    let db = ctx.db.lock().await;
                    let _ = db.update_step_result(step_result);
                }

                // Emit StepCompleted event
                ctx.emit(AirlockEvent::StepCompleted {
                    repo_id: run.repo_id.clone(),
                    run_id: run.id.clone(),
                    job_key: approved_job_key.to_string(),
                    step_name: step.name.clone(),
                    status: step_status_str(res.status).to_string(),
                });

                // Check if we should pause for approval
                if crate::pipeline::should_pause_for_approval(&res) {
                    info!(
                        "Job '{}' paused at step '{}' awaiting approval",
                        approved_job_key, step.name
                    );
                    paused_for_approval = true;
                    break;
                }

                // Check if we should continue
                if !crate::pipeline::should_continue_pipeline(&resolved_step, &res) {
                    error!(
                        "Job '{}' stopped at step '{}' due to failure",
                        approved_job_key, step.name
                    );
                    job_success = false;
                    job_error = res.error.clone();

                    // Mark remaining steps as skipped
                    let db = ctx.db.lock().await;
                    for remaining in step_results.iter_mut() {
                        if remaining.status == StepStatus::Pending {
                            remaining.status = StepStatus::Skipped;
                            let _ = db.update_step_result(remaining);
                        }
                    }
                    break;
                }

                if res.status == StepStatus::Failed {
                    warn!(
                        "Step '{}' in job '{}' failed but continue_on_error=true, continuing",
                        step.name, approved_job_key
                    );
                }
            }
            Err(e) => {
                error!(
                    "Step '{}' in job '{}' execution error: {}",
                    step.name, approved_job_key, e
                );
                job_success = false;
                job_error = Some(e.to_string());

                // Update step as failed
                {
                    let db = ctx.db.lock().await;
                    step_result.status = StepStatus::Failed;
                    step_result.error = Some(e.to_string());
                    let _ = db.update_step_result(step_result);
                }

                // Emit StepCompleted event for failure
                ctx.emit(AirlockEvent::StepCompleted {
                    repo_id: run.repo_id.clone(),
                    run_id: run.id.clone(),
                    job_key: approved_job_key.to_string(),
                    step_name: step.name.clone(),
                    status: "failed".to_string(),
                });
                break;
            }
        }
    }

    // Determine final job status
    let final_job_status = if paused_for_approval {
        JobStatus::AwaitingApproval
    } else if job_success {
        JobStatus::Passed
    } else {
        JobStatus::Failed
    };

    // Clear current step if not paused
    if !paused_for_approval {
        let db = ctx.db.lock().await;
        if let Err(e) = db.update_run_current_step(&run.id, None) {
            warn!("Failed to clear current step: {}", e);
        }
    }

    // Update job status in DB
    finalize_job(
        &ctx,
        &run,
        approved_job_key,
        final_job_status,
        job_error.as_deref(),
    )
    .await;

    // Emit job completed event (only if not paused again)
    if !paused_for_approval {
        let status_str = match final_job_status {
            JobStatus::Passed => "passed",
            JobStatus::Failed => "failed",
            _ => "failed",
        };
        ctx.emit(AirlockEvent::JobCompleted {
            repo_id: run.repo_id.clone(),
            run_id: run.id.clone(),
            job_key: approved_job_key.to_string(),
            status: status_str.to_string(),
        });

        // Resume DAG: check if dependent jobs can now start
        resume_dag_after_job_completion(
            &ctx,
            &run,
            &repo,
            &workflow,
            approved_job_key,
            final_job_status,
        )
        .await;

        // Clean up ephemeral worktrees only (persistent worktree is kept)
        if !keep_worktrees {
            let persistent_wt = ctx.paths.repo_worktree(&run.repo_id);
            if worktree_path != persistent_wt {
                if let Err(e) = airlock_core::remove_worktree(&repo.gate_path, &worktree_path) {
                    warn!("Failed to remove worktree: {}", e);
                }
            }
        }
    }

    // Emit final run-level events based on the state of all jobs
    emit_run_final_status(&ctx, &run).await;
}

/// Finalize a job's status in the database.
async fn finalize_job(
    ctx: &Arc<HandlerContext>,
    run: &airlock_core::Run,
    job_key: &str,
    status: JobStatus,
    error: Option<&str>,
) {
    let db = ctx.db.lock().await;
    if let Ok(jobs) = db.get_job_results_for_run(&run.id) {
        if let Some(job) = jobs.iter().find(|j| j.job_key == job_key) {
            let completed_at = if status == JobStatus::AwaitingApproval {
                None
            } else {
                Some(now_epoch())
            };
            let _ = db.update_job_status(&job.id, status, None, completed_at, error);
        }
    }
}

/// Determine the worktree path for a job that was previously paused.
///
/// Checks the persistent worktree, job-specific extension path, and the
/// standard run worktree path (in that priority order).
fn find_job_worktree(
    paths: &airlock_core::AirlockPaths,
    run: &airlock_core::Run,
    job_key: &str,
) -> std::path::PathBuf {
    // Check persistent worktree first (new default for first/only jobs)
    let persistent_path = paths.repo_worktree(&run.repo_id);
    if persistent_path.exists() {
        return persistent_path;
    }

    let standard_path = paths.run_worktree(&run.repo_id, &run.id);
    let job_specific_path = standard_path.with_extension(job_key);

    // Prefer job-specific path if it exists, otherwise fall back to standard
    if job_specific_path.exists() {
        job_specific_path
    } else {
        standard_path
    }
}

/// Check the state of all jobs and emit appropriate run-level events.
async fn emit_run_final_status(ctx: &Arc<HandlerContext>, run: &airlock_core::Run) {
    let (all_done, any_paused, any_failed, all_passed) = {
        let db = ctx.db.lock().await;
        match db.get_job_results_for_run(&run.id) {
            Ok(jobs) => {
                let all_done = jobs.iter().all(|j| j.status.is_final());
                let any_paused = jobs.iter().any(|j| j.status == JobStatus::AwaitingApproval);
                let any_failed = jobs.iter().any(|j| j.status == JobStatus::Failed);
                let all_passed = jobs.iter().all(|j| j.status == JobStatus::Passed);
                (all_done, any_paused, any_failed, all_passed)
            }
            Err(_) => (false, false, false, false),
        }
    };

    if any_paused && !all_done {
        // Some jobs are still paused — run is waiting for approval
        ctx.emit(AirlockEvent::RunUpdated {
            repo_id: run.repo_id.clone(),
            run_id: run.id.clone(),
            status: "awaiting_approval".to_string(),
        });
    } else if all_done {
        if all_passed {
            info!(
                "Pipeline completed successfully for run {} (resumed)",
                run.id
            );
            ctx.emit(AirlockEvent::RunCompleted {
                repo_id: run.repo_id.clone(),
                run_id: run.id.clone(),
                success: true,
            });
        } else if any_failed {
            error!("Pipeline failed for run {} (resumed)", run.id);
            let db = ctx.db.lock().await;
            let _ = db.update_run_error(&run.id, Some("One or more jobs failed"));
            drop(db);

            ctx.emit(AirlockEvent::RunCompleted {
                repo_id: run.repo_id.clone(),
                run_id: run.id.clone(),
                success: false,
            });
        } else {
            // All done but not all passed and none failed — e.g., all skipped
            ctx.emit(AirlockEvent::RunCompleted {
                repo_id: run.repo_id.clone(),
                run_id: run.id.clone(),
                success: false,
            });
        }
    }
    // If not all done and none paused, there's likely still running jobs — don't emit
}

/// Handle the `apply_patches` method.
///
/// Applies selected patch artifact files to the run's worktree, commits,
/// and updates the run's head_sha in the database.
pub async fn handle_apply_patches(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    let params: ApplyPatchesParams = match parse_params(params, &id) {
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

    // Get repo
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
                format!("Failed to query database: {}", e),
            )
        }
    };

    // Release DB lock before doing I/O
    drop(db);

    // Validate all patch paths are within the artifacts directory
    let artifacts_dir = ctx.paths.artifacts_dir();
    for path in &params.patch_paths {
        let p = std::path::Path::new(path);
        if !p.starts_with(&artifacts_dir) {
            return Response::error(
                id,
                error_codes::INVALID_PARAMS,
                format!("Patch path must be within artifacts directory: {}", path),
            );
        }
    }

    // Create a temporary worktree for applying patches
    let worktree_path = ctx
        .paths
        .run_worktree(&run.repo_id, &format!("{}-patches", run.id));

    if let Err(e) =
        airlock_core::create_run_worktree(&repo.gate_path, &worktree_path, &run.head_sha)
    {
        return Response::error(
            id,
            error_codes::GIT_ERROR,
            format!("Failed to create worktree: {}", e),
        );
    }

    // Configure git user in worktree
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "Airlock"])
        .current_dir(&worktree_path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "airlock@localhost"])
        .current_dir(&worktree_path)
        .output();

    // Apply each patch
    let mut applied_count: u32 = 0;
    let mut patch_errors: Vec<PatchError> = Vec::new();
    let mut applied_titles: Vec<String> = Vec::new();

    for patch_path in &params.patch_paths {
        // Read and parse the patch JSON file
        let content = match std::fs::read_to_string(patch_path) {
            Ok(c) => c,
            Err(e) => {
                patch_errors.push(PatchError {
                    path: patch_path.clone(),
                    error: format!("Failed to read patch file: {}", e),
                });
                continue;
            }
        };

        let parsed: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                patch_errors.push(PatchError {
                    path: patch_path.clone(),
                    error: format!("Failed to parse patch JSON: {}", e),
                });
                continue;
            }
        };

        let title = parsed["title"]
            .as_str()
            .unwrap_or("Untitled Patch")
            .to_string();
        let diff = match parsed["diff"].as_str() {
            Some(d) => d,
            None => {
                patch_errors.push(PatchError {
                    path: patch_path.clone(),
                    error: "Patch JSON missing 'diff' field".to_string(),
                });
                continue;
            }
        };

        // Write diff to temp file and apply
        let temp_dir = std::env::temp_dir();
        let temp_patch = temp_dir.join(format!("airlock-patch-{}.diff", uuid::Uuid::new_v4()));

        if let Err(e) = std::fs::write(&temp_patch, diff) {
            patch_errors.push(PatchError {
                path: patch_path.clone(),
                error: format!("Failed to write temp patch file: {}", e),
            });
            continue;
        }

        // Try git apply --3way first, then fallback to git apply
        let output = std::process::Command::new("git")
            .args(["apply", "--3way"])
            .arg(&temp_patch)
            .current_dir(&worktree_path)
            .output();

        let apply_result = match output {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                debug!(
                    "git apply --3way failed for {}, trying without --3way: {}",
                    title, stderr
                );
                // Fallback: try without --3way
                match std::process::Command::new("git")
                    .args(["apply"])
                    .arg(&temp_patch)
                    .current_dir(&worktree_path)
                    .output()
                {
                    Ok(o2) if o2.status.success() => Ok(()),
                    Ok(o2) => {
                        let stderr2 = String::from_utf8_lossy(&o2.stderr);
                        Err(format!("git apply failed: {}", stderr2))
                    }
                    Err(e) => Err(format!("Failed to execute git apply: {}", e)),
                }
            }
            Err(e) => Err(format!("Failed to execute git apply: {}", e)),
        };

        let _ = std::fs::remove_file(&temp_patch);

        match apply_result {
            Ok(()) => {
                applied_count += 1;
                applied_titles.push(title);
            }
            Err(e) => {
                patch_errors.push(PatchError {
                    path: patch_path.clone(),
                    error: e,
                });
            }
        }
    }

    // If no patches applied, clean up and return
    if applied_count == 0 {
        let _ = airlock_core::remove_worktree(&repo.gate_path, &worktree_path);
        let error_msg = if patch_errors.is_empty() {
            "No patches to apply".to_string()
        } else {
            "All patches failed to apply".to_string()
        };
        let response = ApplyPatchesResult {
            run_id: params.run_id,
            success: false,
            applied_count: 0,
            new_head_sha: None,
            error: Some(error_msg),
            patch_errors,
        };
        return Response::success(id, serde_json::to_value(response).unwrap());
    }

    // Stage all changes
    let stage_output = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(&worktree_path)
        .output();

    if let Err(e) = stage_output {
        let _ = airlock_core::remove_worktree(&repo.gate_path, &worktree_path);
        return Response::error(
            id,
            error_codes::GIT_ERROR,
            format!("Failed to stage changes: {}", e),
        );
    }

    // Check if there are staged changes
    let diff_check = std::process::Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(&worktree_path)
        .output();

    let has_changes = match diff_check {
        Ok(o) => !o.status.success(), // exit code 1 = changes exist
        Err(_) => true,               // assume changes on error
    };

    if !has_changes {
        let _ = airlock_core::remove_worktree(&repo.gate_path, &worktree_path);
        let response = ApplyPatchesResult {
            run_id: params.run_id,
            success: true,
            applied_count,
            new_head_sha: None,
            error: Some("Patches applied but produced no changes".to_string()),
            patch_errors,
        };
        return Response::success(id, serde_json::to_value(response).unwrap());
    }

    // Create commit
    let titles_summary = applied_titles.join(", ");
    let commit_msg = format!("Airlock: applied patches: {}", titles_summary);

    let commit_output = std::process::Command::new("git")
        .args(["commit", "-m", &commit_msg])
        .current_dir(&worktree_path)
        .output();

    if let Ok(ref o) = commit_output {
        if !o.status.success() {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let _ = airlock_core::remove_worktree(&repo.gate_path, &worktree_path);
            return Response::error(
                id,
                error_codes::GIT_ERROR,
                format!("Failed to commit patches: {}", stderr),
            );
        }
    } else if let Err(e) = commit_output {
        let _ = airlock_core::remove_worktree(&repo.gate_path, &worktree_path);
        return Response::error(
            id,
            error_codes::GIT_ERROR,
            format!("Failed to execute git commit: {}", e),
        );
    }

    // Get new HEAD SHA
    let sha_output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&worktree_path)
        .output();

    let new_sha = match sha_output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => {
            let _ = airlock_core::remove_worktree(&repo.gate_path, &worktree_path);
            return Response::error(
                id,
                error_codes::GIT_ERROR,
                "Failed to get new HEAD SHA after commit".to_string(),
            );
        }
    };

    // Update database
    {
        let db = ctx.db.lock().await;
        if let Err(e) = db.update_run_head_sha(&params.run_id, &new_sha) {
            warn!("Failed to update run head_sha: {}", e);
        }
    }

    // Clean up worktree
    if let Err(e) = airlock_core::remove_worktree(&repo.gate_path, &worktree_path) {
        warn!("Failed to remove patches worktree: {}", e);
    }

    // Emit RunUpdated event
    ctx.emit(AirlockEvent::RunUpdated {
        repo_id: run.repo_id.clone(),
        run_id: params.run_id.clone(),
        status: "updated".to_string(),
    });

    info!(
        "Applied {} patches for run {}, new HEAD: {}",
        applied_count, params.run_id, new_sha
    );

    let response = ApplyPatchesResult {
        run_id: params.run_id,
        success: true,
        applied_count,
        new_head_sha: Some(new_sha),
        error: None,
        patch_errors,
    };

    Response::success(id, serde_json::to_value(response).unwrap())
}

/// Handle the `get_run_diff` method.
///
/// Returns the diff between base and head SHA for a run.
pub async fn handle_get_run_diff(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    let params: GetRunDiffParams = match parse_params(params, &id) {
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

    // Get repo
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
                format!("Failed to query database: {}", e),
            )
        }
    };

    // Get diff with per-commit breakdown (handles null SHA for new branches)
    let result = compute_diff_with_commits(&repo.gate_path, &run.base_sha, &run.head_sha);

    // Build per-commit info by pairing commits with their patches
    let commits: Vec<CommitDiffInfo> = if result.commits.len() > 1 {
        result
            .commits
            .iter()
            .zip(result.commit_patches.iter())
            .map(|(commit, patch)| {
                // Parse files_changed and stats from the patch
                let commit_diff = airlock_core::git::get_commit_patch(&repo.gate_path, &commit.sha);
                CommitDiffInfo {
                    sha: commit.sha.clone(),
                    message: commit.message.clone(),
                    author: commit.author.clone(),
                    timestamp: commit.timestamp,
                    patch: patch.clone(),
                    files_changed: commit_diff.files_changed,
                    additions: commit_diff.additions,
                    deletions: commit_diff.deletions,
                }
            })
            .collect()
    } else {
        vec![]
    };

    let response = GetRunDiffResult {
        run_id: params.run_id,
        branch: run.branch,
        base_sha: run.base_sha,
        head_sha: run.head_sha,
        patch: result.diff.patch,
        files_changed: result.diff.files_changed,
        additions: result.diff.additions,
        deletions: result.diff.deletions,
        commits,
    };

    Response::success(id, serde_json::to_value(response).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::HandlerContext;
    use airlock_core::{
        AirlockPaths, Database, JobResult, JobStatus, RefUpdate, Repo, Run, StepResult,
    };
    use std::path::PathBuf;
    use tokio::sync::watch;

    fn create_test_context() -> Arc<HandlerContext> {
        let paths = AirlockPaths::with_root(PathBuf::from("/tmp/airlock-test"));
        let db = Database::open_in_memory().unwrap();
        let (shutdown_tx, _) = watch::channel(false);
        Arc::new(HandlerContext::new(paths, db, shutdown_tx))
    }

    fn create_test_repo(id: &str) -> Repo {
        Repo {
            id: id.to_string(),
            working_path: PathBuf::from("/tmp/test-repo"),
            upstream_url: "git@github.com:user/repo.git".to_string(),
            gate_path: PathBuf::from("/tmp/.airlock/repos/test.git"),
            last_sync: None,
            created_at: 1704067200,
        }
    }

    fn create_test_run(id: &str, repo_id: &str) -> Run {
        Run {
            id: id.to_string(),
            repo_id: repo_id.to_string(),
            ref_updates: vec![RefUpdate {
                ref_name: "refs/heads/feature/test".to_string(),
                old_sha: "abc123".to_string(),
                new_sha: "def456".to_string(),
            }],
            error: None,
            superseded: false,
            created_at: 1704067200,
            branch: "refs/heads/feature/test".to_string(),
            base_sha: "abc123".to_string(),
            head_sha: "def456".to_string(),
            current_step: Some("review".to_string()),
            updated_at: 1704067200,
            workflow_file: "main.yml".to_string(),
            workflow_name: None,
        }
    }

    fn create_test_job_result(id: &str, run_id: &str, job_key: &str) -> JobResult {
        JobResult {
            id: id.to_string(),
            run_id: run_id.to_string(),
            job_key: job_key.to_string(),
            name: None,
            status: JobStatus::Running,
            job_order: 0,
            started_at: Some(1704067200),
            completed_at: None,
            error: None,
        }
    }

    fn create_test_step_result(
        id: &str,
        run_id: &str,
        job_id: &str,
        name: &str,
        status: StepStatus,
    ) -> StepResult {
        StepResult {
            id: id.to_string(),
            run_id: run_id.to_string(),
            job_id: job_id.to_string(),
            name: name.to_string(),
            status,
            step_order: 0,
            exit_code: if status == StepStatus::Passed {
                Some(0)
            } else {
                None
            },
            duration_ms: Some(100),
            error: None,
            started_at: Some(1704067200),
            completed_at: if status == StepStatus::AwaitingApproval {
                None
            } else {
                Some(1704067300)
            },
        }
    }

    #[tokio::test]
    async fn test_approve_step_not_found() {
        let ctx = create_test_context();

        // Set up test data - run without step results
        {
            let db = ctx.db.lock().await;
            let repo = create_test_repo("repo1");
            db.insert_repo(&repo).unwrap();
            let run = create_test_run("run1", "repo1");
            db.insert_run(&run).unwrap();
            let job = create_test_job_result("job1", "run1", "default");
            db.insert_job_result(&job).unwrap();
        }

        let params = serde_json::json!({
            "run_id": "run1",
            "job_key": "default",
            "step_name": "nonexistent"
        });
        let response = handle_approve_step(ctx, params, serde_json::json!(1)).await;

        assert!(response.error.is_some());
        let error = response.error.unwrap();
        assert_eq!(error.code, error_codes::STEP_NOT_FOUND);
    }

    #[tokio::test]
    async fn test_approve_step_wrong_status() {
        let ctx = create_test_context();

        // Set up test data with step in Passed status
        {
            let db = ctx.db.lock().await;
            let repo = create_test_repo("repo1");
            db.insert_repo(&repo).unwrap();
            let run = create_test_run("run1", "repo1");
            db.insert_run(&run).unwrap();
            let job = create_test_job_result("job1", "run1", "default");
            db.insert_job_result(&job).unwrap();
            let step = create_test_step_result("sr1", "run1", "job1", "review", StepStatus::Passed);
            db.insert_step_result(&step).unwrap();
        }

        let params = serde_json::json!({
            "run_id": "run1",
            "job_key": "default",
            "step_name": "review"
        });
        let response = handle_approve_step(ctx, params, serde_json::json!(1)).await;

        assert!(response.error.is_some());
        let error = response.error.unwrap();
        assert_eq!(error.code, error_codes::INVALID_STEP_STATUS);
    }

    #[tokio::test]
    async fn test_approve_step_success() {
        let ctx = create_test_context();

        // Set up test data with step awaiting approval
        {
            let db = ctx.db.lock().await;
            let repo = create_test_repo("repo1");
            db.insert_repo(&repo).unwrap();
            let run = create_test_run("run1", "repo1");
            db.insert_run(&run).unwrap();
            let job = create_test_job_result("job1", "run1", "default");
            db.insert_job_result(&job).unwrap();
            let step = create_test_step_result(
                "sr1",
                "run1",
                "job1",
                "review",
                StepStatus::AwaitingApproval,
            );
            db.insert_step_result(&step).unwrap();
        }

        let params = serde_json::json!({
            "run_id": "run1",
            "job_key": "default",
            "step_name": "review"
        });
        let response = handle_approve_step(ctx.clone(), params, serde_json::json!(1)).await;

        assert!(response.error.is_none());
        let result: ApproveStepResult = serde_json::from_value(response.result.unwrap()).unwrap();
        assert_eq!(result.run_id, "run1");
        assert_eq!(result.job_key, "default");
        assert_eq!(result.step_name, "review");
        assert!(result.success);
        assert_eq!(result.new_step_status, "passed");

        // Verify database was updated
        let db = ctx.db.lock().await;
        let step_results = db.get_step_results_for_run("run1").unwrap();
        let review_step = step_results.iter().find(|s| s.name == "review").unwrap();
        assert_eq!(review_step.status, StepStatus::Passed);

        // Verify job status was updated to Running
        let jobs = db.get_job_results_for_run("run1").unwrap();
        let job = jobs.iter().find(|j| j.job_key == "default").unwrap();
        assert_eq!(job.status, JobStatus::Running);
    }

    #[tokio::test]
    async fn test_approve_step_updates_job_status_to_running() {
        let ctx = create_test_context();

        // Set up: job in AwaitingApproval, step in AwaitingApproval
        {
            let db = ctx.db.lock().await;
            let repo = create_test_repo("repo1");
            db.insert_repo(&repo).unwrap();
            let run = create_test_run("run1", "repo1");
            db.insert_run(&run).unwrap();
            let mut job = create_test_job_result("job1", "run1", "default");
            job.status = JobStatus::AwaitingApproval;
            db.insert_job_result(&job).unwrap();
            let step = create_test_step_result(
                "sr1",
                "run1",
                "job1",
                "review",
                StepStatus::AwaitingApproval,
            );
            db.insert_step_result(&step).unwrap();
        }

        let params = serde_json::json!({
            "run_id": "run1",
            "job_key": "default",
            "step_name": "review"
        });
        let response = handle_approve_step(ctx.clone(), params, serde_json::json!(1)).await;

        assert!(response.error.is_none());

        // Verify job was set back to Running
        let db = ctx.db.lock().await;
        let jobs = db.get_job_results_for_run("run1").unwrap();
        let job = jobs.iter().find(|j| j.job_key == "default").unwrap();
        assert_eq!(job.status, JobStatus::Running);
    }

    #[tokio::test]
    async fn test_get_run_diff_run_not_found() {
        let ctx = create_test_context();

        let params = serde_json::json!({
            "run_id": "nonexistent"
        });
        let response = handle_get_run_diff(ctx, params, serde_json::json!(1)).await;

        assert!(response.error.is_some());
        let error = response.error.unwrap();
        assert_eq!(error.code, error_codes::RUN_NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_run_diff_success() {
        let ctx = create_test_context();

        // Set up test data
        {
            let db = ctx.db.lock().await;
            let repo = create_test_repo("repo1");
            db.insert_repo(&repo).unwrap();
            let run = create_test_run("run1", "repo1");
            db.insert_run(&run).unwrap();
        }

        let params = serde_json::json!({
            "run_id": "run1"
        });
        let response = handle_get_run_diff(ctx, params, serde_json::json!(1)).await;

        // Should succeed even if git diff fails (returns empty patch)
        assert!(response.error.is_none());
        let result: GetRunDiffResult = serde_json::from_value(response.result.unwrap()).unwrap();
        assert_eq!(result.run_id, "run1");
        assert_eq!(result.branch, "refs/heads/feature/test");
        assert_eq!(result.base_sha, "abc123");
        assert_eq!(result.head_sha, "def456");
    }

    #[tokio::test]
    async fn test_emit_run_final_status_all_passed() {
        let ctx = create_test_context();

        {
            let db = ctx.db.lock().await;
            let repo = create_test_repo("repo1");
            db.insert_repo(&repo).unwrap();
            let run = create_test_run("run1", "repo1");
            db.insert_run(&run).unwrap();

            let mut job1 = create_test_job_result("job1", "run1", "lint");
            job1.status = JobStatus::Passed;
            db.insert_job_result(&job1).unwrap();

            let mut job2 = create_test_job_result("job2", "run1", "test");
            job2.status = JobStatus::Passed;
            job2.job_order = 1;
            db.insert_job_result(&job2).unwrap();
        }

        let run = {
            let db = ctx.db.lock().await;
            db.get_run("run1").unwrap().unwrap()
        };

        // Subscribe to events before emitting
        let mut rx = ctx.subscribe();

        emit_run_final_status(&ctx, &run).await;

        // Should receive RunCompleted with success=true
        let event = rx.try_recv().unwrap();
        match event {
            AirlockEvent::RunCompleted {
                run_id, success, ..
            } => {
                assert_eq!(run_id, "run1");
                assert!(success);
            }
            other => panic!("Expected RunCompleted, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_emit_run_final_status_some_failed() {
        let ctx = create_test_context();

        {
            let db = ctx.db.lock().await;
            let repo = create_test_repo("repo1");
            db.insert_repo(&repo).unwrap();
            let run = create_test_run("run1", "repo1");
            db.insert_run(&run).unwrap();

            let mut job1 = create_test_job_result("job1", "run1", "lint");
            job1.status = JobStatus::Passed;
            db.insert_job_result(&job1).unwrap();

            let mut job2 = create_test_job_result("job2", "run1", "test");
            job2.status = JobStatus::Failed;
            job2.job_order = 1;
            db.insert_job_result(&job2).unwrap();
        }

        let run = {
            let db = ctx.db.lock().await;
            db.get_run("run1").unwrap().unwrap()
        };

        let mut rx = ctx.subscribe();
        emit_run_final_status(&ctx, &run).await;

        let event = rx.try_recv().unwrap();
        match event {
            AirlockEvent::RunCompleted {
                run_id, success, ..
            } => {
                assert_eq!(run_id, "run1");
                assert!(!success);
            }
            other => panic!("Expected RunCompleted, got {:?}", other),
        }
    }

    // =========================================================================
    // apply_patches handler tests
    // =========================================================================

    /// Create a real bare gate repo with an initial commit containing `file.txt`.
    /// Returns (temp_dir, gate_path, head_sha).
    fn setup_gate_repo() -> (tempfile::TempDir, std::path::PathBuf, String) {
        let temp = tempfile::TempDir::new().unwrap();

        // Create a regular working repo first
        let work_path = temp.path().join("work");
        std::fs::create_dir_all(&work_path).unwrap();

        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&work_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&work_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&work_path)
            .output()
            .unwrap();

        // Create initial file and commit
        std::fs::write(work_path.join("file.txt"), "initial content\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "file.txt"])
            .current_dir(&work_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(&work_path)
            .output()
            .unwrap();

        // Get the commit SHA
        let sha_output = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&work_path)
            .output()
            .unwrap();
        let head_sha = String::from_utf8_lossy(&sha_output.stdout)
            .trim()
            .to_string();

        // Create a bare clone to act as the gate
        let gate_path = temp.path().join("gate.git");
        std::process::Command::new("git")
            .args([
                "clone",
                "--bare",
                work_path.to_str().unwrap(),
                gate_path.to_str().unwrap(),
            ])
            .output()
            .unwrap();

        (temp, gate_path, head_sha)
    }

    /// Helper to create a test context rooted in a temp dir so artifacts_dir is real.
    fn create_real_test_context(root: &std::path::Path) -> Arc<HandlerContext> {
        let paths = AirlockPaths::with_root(root.to_path_buf());
        let db = Database::open_in_memory().unwrap();
        let (shutdown_tx, _) = watch::channel(false);
        Arc::new(HandlerContext::new(paths, db, shutdown_tx))
    }

    #[tokio::test]
    async fn test_apply_patches_run_not_found() {
        let ctx = create_test_context();

        let params = serde_json::json!({
            "run_id": "nonexistent",
            "patch_paths": []
        });
        let response = handle_apply_patches(ctx, params, serde_json::json!(1)).await;

        assert!(response.error.is_some());
        assert_eq!(response.error.unwrap().code, error_codes::RUN_NOT_FOUND);
    }

    #[tokio::test]
    async fn test_apply_patches_invalid_path_outside_artifacts() {
        let ctx = create_test_context();

        {
            let db = ctx.db.lock().await;
            let repo = create_test_repo("repo1");
            db.insert_repo(&repo).unwrap();
            let run = create_test_run("run1", "repo1");
            db.insert_run(&run).unwrap();
        }

        let params = serde_json::json!({
            "run_id": "run1",
            "patch_paths": ["/etc/passwd"]
        });
        let response = handle_apply_patches(ctx, params, serde_json::json!(1)).await;

        assert!(response.error.is_some());
        let err = response.error.unwrap();
        assert_eq!(err.code, error_codes::INVALID_PARAMS);
        assert!(err.message.contains("artifacts directory"));
    }

    #[tokio::test]
    async fn test_apply_patches_success() {
        let (temp, gate_path, head_sha) = setup_gate_repo();
        let airlock_root = temp.path().join("airlock-root");
        std::fs::create_dir_all(&airlock_root).unwrap();

        let ctx = create_real_test_context(&airlock_root);

        // Create a repo record pointing to the real gate
        let repo = Repo {
            id: "repo1".to_string(),
            working_path: PathBuf::from("/tmp/unused"),
            upstream_url: "git@github.com:user/repo.git".to_string(),
            gate_path: gate_path.clone(),
            last_sync: None,
            created_at: 1704067200,
        };

        let run = Run {
            id: "run1".to_string(),
            repo_id: "repo1".to_string(),
            ref_updates: vec![RefUpdate {
                ref_name: "refs/heads/main".to_string(),
                old_sha: "0000000".to_string(),
                new_sha: head_sha.clone(),
            }],
            error: None,
            superseded: false,
            created_at: 1704067200,
            branch: "main".to_string(),
            base_sha: "0000000".to_string(),
            head_sha: head_sha.clone(),
            current_step: None,
            updated_at: 1704067200,
            workflow_file: "main.yml".to_string(),
            workflow_name: None,
        };

        {
            let db = ctx.db.lock().await;
            db.insert_repo(&repo).unwrap();
            db.insert_run(&run).unwrap();
        }

        // Create a patch artifact inside the artifacts dir
        let artifacts_dir = ctx.paths.artifacts_dir();
        let patches_dir = artifacts_dir.join("repo1").join("run1").join("patches");
        std::fs::create_dir_all(&patches_dir).unwrap();

        let patch_json = r#"{
            "title": "Fix typo",
            "explanation": "Fix a typo in file.txt",
            "diff": "--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-initial content\n+fixed content\n"
        }"#;
        let patch_path = patches_dir.join("patch1.json");
        std::fs::write(&patch_path, patch_json).unwrap();

        // Subscribe to events before the call
        let mut rx = ctx.subscribe();

        let params = serde_json::json!({
            "run_id": "run1",
            "patch_paths": [patch_path.to_str().unwrap()]
        });
        let response = handle_apply_patches(ctx.clone(), params, serde_json::json!(1)).await;

        // Verify success response
        assert!(
            response.error.is_none(),
            "Expected success, got error: {:?}",
            response.error
        );
        let result: ApplyPatchesResult = serde_json::from_value(response.result.unwrap()).unwrap();
        assert!(result.success);
        assert_eq!(result.applied_count, 1);
        assert!(result.new_head_sha.is_some());
        assert!(result.patch_errors.is_empty());

        let new_sha = result.new_head_sha.unwrap();
        assert_ne!(
            new_sha, head_sha,
            "HEAD should have changed after applying patch"
        );

        // Verify the DB was updated with the new SHA
        {
            let db = ctx.db.lock().await;
            let updated_run = db.get_run("run1").unwrap().unwrap();
            assert_eq!(updated_run.head_sha, new_sha);
        }

        // Verify a RunUpdated event was emitted
        let event = rx.try_recv().unwrap();
        match event {
            AirlockEvent::RunUpdated { run_id, .. } => {
                assert_eq!(run_id, "run1");
            }
            other => panic!("Expected RunUpdated, got {:?}", other),
        }

        // Verify the new commit exists in the gate repo by checking the log
        let log_output = std::process::Command::new("git")
            .args(["log", "--oneline", &new_sha])
            .current_dir(&gate_path)
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&log_output.stdout);
        assert!(
            log.contains("Airlock: applied patches"),
            "Commit message should contain 'Airlock: applied patches', got: {}",
            log
        );
    }

    #[tokio::test]
    async fn test_apply_patches_bad_diff_returns_patch_error() {
        let (temp, gate_path, head_sha) = setup_gate_repo();
        let airlock_root = temp.path().join("airlock-root");
        std::fs::create_dir_all(&airlock_root).unwrap();

        let ctx = create_real_test_context(&airlock_root);

        let repo = Repo {
            id: "repo1".to_string(),
            working_path: PathBuf::from("/tmp/unused"),
            upstream_url: "git@github.com:user/repo.git".to_string(),
            gate_path: gate_path.clone(),
            last_sync: None,
            created_at: 1704067200,
        };

        let run = Run {
            id: "run1".to_string(),
            repo_id: "repo1".to_string(),
            ref_updates: vec![RefUpdate {
                ref_name: "refs/heads/main".to_string(),
                old_sha: "0000000".to_string(),
                new_sha: head_sha.clone(),
            }],
            error: None,
            superseded: false,
            created_at: 1704067200,
            branch: "main".to_string(),
            base_sha: "0000000".to_string(),
            head_sha: head_sha.clone(),
            current_step: None,
            updated_at: 1704067200,
            workflow_file: "main.yml".to_string(),
            workflow_name: None,
        };

        {
            let db = ctx.db.lock().await;
            db.insert_repo(&repo).unwrap();
            db.insert_run(&run).unwrap();
        }

        // Create a patch with garbage diff that won't apply
        let artifacts_dir = ctx.paths.artifacts_dir();
        let patches_dir = artifacts_dir.join("repo1").join("run1").join("patches");
        std::fs::create_dir_all(&patches_dir).unwrap();

        let bad_patch = r#"{
            "title": "Bad patch",
            "explanation": "This diff is nonsense",
            "diff": "--- a/nonexistent.txt\n+++ b/nonexistent.txt\n@@ -1,5 +1,5 @@\n-line that does not exist\n+replaced\n"
        }"#;
        let patch_path = patches_dir.join("bad.json");
        std::fs::write(&patch_path, bad_patch).unwrap();

        let params = serde_json::json!({
            "run_id": "run1",
            "patch_paths": [patch_path.to_str().unwrap()]
        });
        let response = handle_apply_patches(ctx.clone(), params, serde_json::json!(1)).await;

        // Should return success at the JSON-RPC level but with success=false in the result
        assert!(response.error.is_none());
        let result: ApplyPatchesResult = serde_json::from_value(response.result.unwrap()).unwrap();
        assert!(!result.success);
        assert_eq!(result.applied_count, 0);
        assert!(result.new_head_sha.is_none());
        assert_eq!(result.patch_errors.len(), 1);
        assert!(result.patch_errors[0].error.contains("git apply failed"));

        // Verify DB was NOT updated (head_sha still the original)
        {
            let db = ctx.db.lock().await;
            let unchanged_run = db.get_run("run1").unwrap().unwrap();
            assert_eq!(unchanged_run.head_sha, head_sha);
        }
    }

    #[tokio::test]
    async fn test_apply_patches_multiple_with_partial_failure() {
        let (temp, gate_path, head_sha) = setup_gate_repo();
        let airlock_root = temp.path().join("airlock-root");
        std::fs::create_dir_all(&airlock_root).unwrap();

        let ctx = create_real_test_context(&airlock_root);

        let repo = Repo {
            id: "repo1".to_string(),
            working_path: PathBuf::from("/tmp/unused"),
            upstream_url: "git@github.com:user/repo.git".to_string(),
            gate_path: gate_path.clone(),
            last_sync: None,
            created_at: 1704067200,
        };

        let run = Run {
            id: "run1".to_string(),
            repo_id: "repo1".to_string(),
            ref_updates: vec![RefUpdate {
                ref_name: "refs/heads/main".to_string(),
                old_sha: "0000000".to_string(),
                new_sha: head_sha.clone(),
            }],
            error: None,
            superseded: false,
            created_at: 1704067200,
            branch: "main".to_string(),
            base_sha: "0000000".to_string(),
            head_sha: head_sha.clone(),
            current_step: None,
            updated_at: 1704067200,
            workflow_file: "main.yml".to_string(),
            workflow_name: None,
        };

        {
            let db = ctx.db.lock().await;
            db.insert_repo(&repo).unwrap();
            db.insert_run(&run).unwrap();
        }

        let artifacts_dir = ctx.paths.artifacts_dir();
        let patches_dir = artifacts_dir.join("repo1").join("run1").join("patches");
        std::fs::create_dir_all(&patches_dir).unwrap();

        // Good patch
        let good_patch = r#"{
            "title": "Fix content",
            "explanation": "Fixes file.txt",
            "diff": "--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-initial content\n+updated content\n"
        }"#;
        let good_path = patches_dir.join("good.json");
        std::fs::write(&good_path, good_patch).unwrap();

        // Bad patch (missing diff field)
        let bad_patch = r#"{
            "title": "Missing diff",
            "explanation": "No diff here"
        }"#;
        let bad_path = patches_dir.join("bad.json");
        std::fs::write(&bad_path, bad_patch).unwrap();

        let params = serde_json::json!({
            "run_id": "run1",
            "patch_paths": [
                good_path.to_str().unwrap(),
                bad_path.to_str().unwrap()
            ]
        });
        let response = handle_apply_patches(ctx.clone(), params, serde_json::json!(1)).await;

        assert!(response.error.is_none());
        let result: ApplyPatchesResult = serde_json::from_value(response.result.unwrap()).unwrap();

        // The good patch applied; the bad one errored
        assert!(result.success);
        assert_eq!(result.applied_count, 1);
        assert!(result.new_head_sha.is_some());
        assert_eq!(result.patch_errors.len(), 1);
        assert!(result.patch_errors[0]
            .error
            .contains("missing 'diff' field"));
    }

    #[tokio::test]
    async fn test_emit_run_final_status_some_paused() {
        let ctx = create_test_context();

        {
            let db = ctx.db.lock().await;
            let repo = create_test_repo("repo1");
            db.insert_repo(&repo).unwrap();
            let run = create_test_run("run1", "repo1");
            db.insert_run(&run).unwrap();

            let mut job1 = create_test_job_result("job1", "run1", "lint");
            job1.status = JobStatus::Passed;
            db.insert_job_result(&job1).unwrap();

            let mut job2 = create_test_job_result("job2", "run1", "deploy");
            job2.status = JobStatus::AwaitingApproval;
            job2.job_order = 1;
            db.insert_job_result(&job2).unwrap();
        }

        let run = {
            let db = ctx.db.lock().await;
            db.get_run("run1").unwrap().unwrap()
        };

        let mut rx = ctx.subscribe();
        emit_run_final_status(&ctx, &run).await;

        let event = rx.try_recv().unwrap();
        match event {
            AirlockEvent::RunUpdated { run_id, status, .. } => {
                assert_eq!(run_id, "run1");
                assert_eq!(status, "awaiting_approval");
            }
            other => panic!("Expected RunUpdated, got {:?}", other),
        }
    }
}
