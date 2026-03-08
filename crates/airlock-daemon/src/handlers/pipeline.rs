//! Pipeline handlers.
//!
//! Handles pipeline execution and related operations.
//!
//! This module uses the workflow/job/step architecture where:
//! 1. Workflows are defined in .airlock/workflows/*.yml
//! 2. Each workflow contains jobs (which may run in parallel via `needs:` DAG)
//! 3. Each job contains steps that run sequentially in a shared worktree
//! 4. Steps can pause for approval with `require-approval: true`

use super::HandlerContext;
use crate::ipc::AirlockEvent;
use crate::pipeline::LogStreamCallback;
use crate::stage_loader::StageLoader;
use crate::worktree_pool::PoolLease;
use airlock_core::{
    filter_workflows_for_branch, load_workflows_from_tree, validate_job_dag, ApprovalMode,
    JobConfig, JobResult, JobStatus, RefUpdate, Repo, Run, StepDefinition, StepResult, StepStatus,
    WorkflowConfig,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// Extract the primary branch name from ref updates.
///
/// For multiple ref updates, this returns the first non-deletion branch.
/// Branch refs are expected in the format "refs/heads/<branch>".
pub fn extract_branch_name(ref_updates: &[RefUpdate]) -> Option<String> {
    for update in ref_updates {
        // Skip deletions (where new_sha is all zeros)
        if update.new_sha == "0000000000000000000000000000000000000000" {
            continue;
        }

        // Extract branch name from ref (e.g., "refs/heads/main" -> "main")
        if let Some(branch) = update.ref_name.strip_prefix("refs/heads/") {
            return Some(branch.to_string());
        }
    }
    None
}

/// Load matching workflow configs from the pushed commit in the gate bare repo.
///
/// Reads `.airlock/workflows/*.yml` from the pushed commit (`head_sha`) in the gate,
/// filters by branch, and returns matching workflows.
pub fn load_workflows_for_run(
    gate_path: &Path,
    head_sha: &str,
    branch: Option<&str>,
) -> Result<Vec<(String, WorkflowConfig)>, String> {
    let all_workflows = load_workflows_from_tree(gate_path, head_sha).map_err(|e| {
        format!(
            "Failed to load workflows from commit {} in gate: {}. Run 'airlock init' to create them.",
            &head_sha[..8.min(head_sha.len())],
            e
        )
    })?;

    if all_workflows.is_empty() {
        return Err(format!(
            ".airlock/workflows/ not found or empty at commit {} in gate. Run 'airlock init' to create it.",
            &head_sha[..8.min(head_sha.len())]
        ));
    }

    // Filter by branch if available
    let matching = if let Some(branch) = branch {
        let filtered = filter_workflows_for_branch(all_workflows, branch);
        if filtered.is_empty() {
            return Err(format!(
                "No workflow matched branch '{}'. Check your .airlock/workflows/*.yml trigger filters.",
                branch
            ));
        }
        filtered
    } else {
        all_workflows
    };

    Ok(matching)
}

/// Execute all matching workflows for a push event.
///
/// This is the top-level entry point called from the push handler.
/// For each matching workflow, it creates job/step records and executes the DAG.
///
/// The `cancel` token is monitored throughout execution. When cancelled
/// (e.g. by a newer push superseding this run), the pipeline stops as
/// soon as the current step finishes and marks remaining work as skipped.
pub async fn execute_pipeline(
    ctx: Arc<HandlerContext>,
    mut run: Run,
    repo: Repo,
    cancel: CancellationToken,
) {
    info!("Starting pipeline execution for run {}", run.id);

    // Extract branch name from ref updates (for config selection)
    let branch = extract_branch_name(&run.ref_updates);
    if let Some(ref b) = branch {
        debug!("Pipeline branch: {}", b);
        if run.branch.is_empty() || run.branch == "refs/heads/" {
            run.branch = format!("refs/heads/{}", b);
        }
    }

    // Extract base_sha and head_sha from ref_updates if not already set
    if run.head_sha.is_empty() {
        if let Some(update) = run
            .ref_updates
            .iter()
            .find(|r| r.new_sha != "0000000000000000000000000000000000000000")
        {
            run.head_sha = update.new_sha.clone();
            run.base_sha = update.old_sha.clone();
        }
    }

    // Load matching workflows from the pushed commit
    let workflows = match load_workflows_for_run(&repo.gate_path, &run.head_sha, branch.as_deref())
    {
        Ok(w) => w,
        Err(e) => {
            error!("Failed to load workflows: {}", e);
            let db = ctx.db.lock().await;
            if let Err(db_err) = db.update_run_error(&run.id, Some(&e)) {
                error!("Failed to update run error: {}", db_err);
            }
            return;
        }
    };

    // For the current run, use the first matching workflow.
    // (Multiple workflow support creates multiple runs in the push handler;
    // by the time we get here, this run is already associated with one workflow.)
    let (workflow_file, workflow) = &workflows[0];

    // Update run with workflow info if not already set
    if run.workflow_file.is_empty() {
        run.workflow_file = workflow_file.clone();
        run.workflow_name = workflow.name.clone();
    }

    info!(
        "Executing workflow '{}' (file: {}) with {} job(s)",
        workflow.name.as_deref().unwrap_or("unnamed"),
        workflow_file,
        workflow.jobs.len()
    );

    // Validate job DAG and get execution waves
    let waves = match validate_job_dag(&workflow.jobs) {
        Ok(w) => w,
        Err(e) => {
            error!("Invalid job DAG in workflow '{}': {}", workflow_file, e);
            let db = ctx.db.lock().await;
            let _ = db.update_run_error(&run.id, Some(&format!("Invalid job DAG: {}", e)));
            ctx.emit(AirlockEvent::RunCompleted {
                repo_id: run.repo_id.clone(),
                run_id: run.id.clone(),
                success: false,
                branch: run.branch.clone(),
            });
            return;
        }
    };

    info!("Job DAG has {} wave(s): {:?}", waves.len(), waves);

    // Create job and step result records in the DB
    let job_results = create_job_and_step_records(&ctx, &run, workflow, &waves).await;
    let job_results = match job_results {
        Ok(jr) => jr,
        Err(e) => {
            error!("Failed to create job/step records: {}", e);
            let db = ctx.db.lock().await;
            let _ = db.update_run_error(&run.id, Some(&e));
            return;
        }
    };

    // Check cancellation before starting DAG execution
    if cancel.is_cancelled() {
        mark_run_cancelled(&ctx, &run).await;
        return;
    }

    // Execute the workflow DAG
    execute_workflow_dag(&ctx, &run, &repo, workflow, &waves, &job_results, &cancel).await;
}

/// Create JobResult and StepResult records in the database for all jobs and steps.
///
/// Returns a map of job_key -> (job_result_id, Vec<step_result_ids>).
async fn create_job_and_step_records(
    ctx: &Arc<HandlerContext>,
    run: &Run,
    workflow: &WorkflowConfig,
    waves: &[Vec<String>],
) -> Result<HashMap<String, String>, String> {
    let db = ctx.db.lock().await;
    let mut job_id_map = HashMap::new();

    // Compute job_order from waves (flat index across all waves)
    let mut job_order = 0i32;
    for wave in waves {
        for job_key in wave {
            let job_config = workflow.jobs.get(job_key).unwrap();
            let job_id = uuid::Uuid::new_v4().to_string();

            let job_result = JobResult {
                id: job_id.clone(),
                run_id: run.id.clone(),
                job_key: job_key.clone(),
                name: job_config.name.clone(),
                status: JobStatus::Pending,
                job_order,
                started_at: None,
                completed_at: None,
                error: None,
                worktree_path: None,
            };

            if let Err(e) = db.insert_job_result(&job_result) {
                return Err(format!(
                    "Failed to insert job result for '{}': {}",
                    job_key, e
                ));
            }

            // Create step results for this job
            for (step_idx, step) in job_config.steps.iter().enumerate() {
                let step_result = StepResult {
                    id: uuid::Uuid::new_v4().to_string(),
                    run_id: run.id.clone(),
                    job_id: job_id.clone(),
                    name: step.name.clone(),
                    status: StepStatus::Pending,
                    step_order: step_idx as i32,
                    exit_code: None,
                    duration_ms: None,
                    error: None,
                    started_at: None,
                    completed_at: None,
                };

                if let Err(e) = db.insert_step_result(&step_result) {
                    return Err(format!(
                        "Failed to insert step result for '{}': {}",
                        step.name, e
                    ));
                }
            }

            job_id_map.insert(job_key.clone(), job_id);
            job_order += 1;
        }
    }

    Ok(job_id_map)
}

/// Execute the workflow's job DAG wave by wave.
///
/// Jobs within a wave execute in parallel. Waves execute sequentially.
/// Checks `cancel` between waves and before each job.
///
/// Worktrees are acquired from the pool and released after completion,
/// except for paused (AwaitingApproval) jobs.
async fn execute_workflow_dag(
    ctx: &Arc<HandlerContext>,
    run: &Run,
    repo: &Repo,
    workflow: &WorkflowConfig,
    waves: &[Vec<String>],
    job_id_map: &HashMap<String, String>,
    cancel: &CancellationToken,
) {
    // Create run artifacts directory
    if let Err(e) = crate::pipeline::create_run_artifacts_dir(&ctx.paths, &run.repo_id, &run.id) {
        warn!("Failed to create run artifacts directory: {}", e);
    }

    // Track job statuses and leases for worktree inheritance and cleanup
    let mut job_statuses: HashMap<String, JobStatus> = HashMap::new();
    // Maps job_key → PoolLease for jobs that acquired a worktree from the pool
    let mut job_leases: HashMap<String, PoolLease> = HashMap::new();
    // Maps job_key → PathBuf for worktree path lookup (including inherited)
    let mut job_worktrees: HashMap<String, PathBuf> = HashMap::new();

    let mut cancelled = false;
    for (wave_idx, wave) in waves.iter().enumerate() {
        // Check cancellation before each wave
        if cancel.is_cancelled() {
            info!("Run {} cancelled before wave {}", run.id, wave_idx + 1);
            for remaining_wave in &waves[wave_idx..] {
                for jk in remaining_wave {
                    skip_job(ctx, run, jk, job_id_map, &mut job_statuses).await;
                }
            }
            mark_run_cancelled(ctx, run).await;
            cancelled = true;
            break;
        }

        debug!(
            "Executing wave {}/{}: {:?}",
            wave_idx + 1,
            waves.len(),
            wave
        );

        if wave.len() == 1 {
            let job_key = &wave[0];
            let job_config = workflow.jobs.get(job_key).unwrap();

            if should_skip_job(job_key, job_config, &job_statuses) {
                skip_job(ctx, run, job_key, job_id_map, &mut job_statuses).await;
                continue;
            }

            let (worktree_path, lease) =
                match resolve_job_worktree(ctx, job_key, run, repo, job_id_map).await {
                    Ok(result) => result,
                    Err(e) => {
                        fail_job_worktree(ctx, run, job_key, job_id_map, &mut job_statuses, &e)
                            .await;
                        continue;
                    }
                };

            let status = execute_single_job(
                ctx,
                run,
                repo,
                job_key,
                job_config,
                job_id_map,
                &worktree_path,
                cancel,
            )
            .await;

            job_worktrees.insert(job_key.clone(), worktree_path);
            if let Some(l) = lease {
                job_leases.insert(job_key.clone(), l);
            }
            job_statuses.insert(job_key.clone(), status);
        } else {
            let mut join_set = tokio::task::JoinSet::new();
            let mut wave_jobs: Vec<(String, PathBuf, Option<PoolLease>)> = Vec::new();

            for job_key in wave {
                let job_config = workflow.jobs.get(job_key).unwrap();

                if should_skip_job(job_key, job_config, &job_statuses) {
                    skip_job(ctx, run, job_key, job_id_map, &mut job_statuses).await;
                    continue;
                }

                match resolve_job_worktree(ctx, job_key, run, repo, job_id_map).await {
                    Ok((worktree_path, lease)) => {
                        wave_jobs.push((job_key.clone(), worktree_path, lease));
                    }
                    Err(e) => {
                        fail_job_worktree(ctx, run, job_key, job_id_map, &mut job_statuses, &e)
                            .await;
                    }
                }
            }

            let spawned_keys: Vec<String> = wave_jobs.iter().map(|(k, _, _)| k.clone()).collect();

            for (job_key, worktree_path, lease) in wave_jobs {
                if let Some(l) = lease {
                    job_leases.insert(job_key.clone(), l);
                }
                job_worktrees.insert(job_key.clone(), worktree_path.clone());

                let ctx = ctx.clone();
                let run = run.clone();
                let repo = repo.clone();
                let job_config = workflow.jobs.get(&job_key).unwrap().clone();
                let job_id_map = job_id_map.clone();
                let cancel = cancel.clone();

                join_set.spawn(async move {
                    let status = execute_single_job(
                        &ctx,
                        &run,
                        &repo,
                        &job_key,
                        &job_config,
                        &job_id_map,
                        &worktree_path,
                        &cancel,
                    )
                    .await;
                    (job_key, status)
                });
            }

            while let Some(result) = join_set.join_next().await {
                match result {
                    Ok((job_key, status)) => {
                        job_statuses.insert(job_key, status);
                    }
                    Err(e) => {
                        error!("Job task panicked: {}", e);
                    }
                }
            }

            for key in &spawned_keys {
                if !job_statuses.contains_key(key) {
                    error!("Job '{}' panicked — marking as failed", key);
                    job_statuses.insert(key.clone(), JobStatus::Failed);
                    if let Some(job_id) = job_id_map.get(key) {
                        let db = ctx.db.lock().await;
                        let _ = db.update_job_status(
                            job_id,
                            JobStatus::Failed,
                            None,
                            Some(now_epoch()),
                            Some("Internal error: job task panicked"),
                        );
                    }
                }
            }
        }
    }

    release_leases_and_cleanup(ctx, run, repo, &job_leases, &job_worktrees, &job_statuses).await;

    // Emit final run result (skip if already emitted by mark_run_cancelled)
    if !cancelled {
        emit_run_final_status(ctx, run).await;
    }
}

/// Determine the worktree path for a job by acquiring a pool slot.
///
/// Every job gets its own pool lease to avoid concurrent access to the same
/// worktree in fan-out DAGs (e.g. `build -> {lint, test}`).
///
/// Returns `(PathBuf, Option<PoolLease>)` — the lease is `None` when
/// pool acquisition fails and an ephemeral worktree is used as fallback.
///
/// Returns `Err` when both pool acquisition and ephemeral fallback fail.
pub(super) async fn resolve_job_worktree(
    ctx: &Arc<HandlerContext>,
    job_key: &str,
    run: &Run,
    repo: &Repo,
    job_id_map: &HashMap<String, String>,
) -> Result<(PathBuf, Option<PoolLease>), String> {
    // Reload head_sha from DB to pick up updates from prior jobs (e.g. freeze).
    // The in-memory `run.head_sha` may be stale if an earlier job updated it.
    let current_head_sha = {
        let db = ctx.db.lock().await;
        match db.get_run(&run.id) {
            Ok(Some(r)) => r.head_sha,
            _ => run.head_sha.clone(),
        }
    };

    // Acquire from pool
    match ctx
        .worktree_pool
        .acquire(&run.repo_id, &repo.gate_path, &current_head_sha, &ctx.paths)
        .await
    {
        Ok(lease) => {
            let path = lease.path.clone();
            // Store worktree_path in DB for crash recovery
            if let Some(job_id) = job_id_map.get(job_key) {
                let db = ctx.db.lock().await;
                if let Err(e) = db.update_job_worktree_path(job_id, &path.to_string_lossy()) {
                    warn!(
                        "Failed to persist worktree path for job '{}': {} (crash recovery may not find this worktree)",
                        job_key, e
                    );
                }
            }
            debug!(
                "Job '{}' acquired pool worktree slot {} at {:?}",
                job_key, lease.slot_index, path
            );
            Ok((path, Some(lease)))
        }
        Err(e) => {
            error!(
                "Failed to acquire pool worktree for job '{}': {}, falling back to ephemeral",
                job_key, e
            );
            let ephemeral_wt = ctx
                .paths
                .run_worktree(&run.repo_id, &run.id)
                .with_extension(job_key);
            match airlock_core::create_run_worktree(
                &repo.gate_path,
                &ephemeral_wt,
                &current_head_sha,
            ) {
                Ok(()) => Ok((ephemeral_wt, None)),
                Err(e2) => {
                    error!(
                        "Ephemeral worktree fallback also failed for job '{}': {}",
                        job_key, e2
                    );
                    Err(format!(
                        "Failed to acquire worktree for job '{}': pool error: {}, ephemeral error: {}",
                        job_key, e, e2
                    ))
                }
            }
        }
    }
}

/// Check if a job should be skipped due to failed dependencies.
pub(super) fn should_skip_job(
    _job_key: &str,
    job_config: &JobConfig,
    job_statuses: &HashMap<String, JobStatus>,
) -> bool {
    for dep in job_config.needs.iter() {
        match job_statuses.get(dep.as_str()) {
            Some(JobStatus::Failed) | Some(JobStatus::Skipped) => return true,
            _ => {}
        }
    }
    false
}

/// Mark a job and its steps as skipped.
pub(super) async fn skip_job(
    ctx: &Arc<HandlerContext>,
    run: &Run,
    job_key: &str,
    job_id_map: &HashMap<String, String>,
    job_statuses: &mut HashMap<String, JobStatus>,
) {
    info!("Skipping job '{}' due to failed dependency", job_key);

    let job_id = match job_id_map.get(job_key) {
        Some(id) => id.clone(),
        None => return,
    };

    let db = ctx.db.lock().await;

    // Mark job as skipped
    if let Err(e) = db.update_job_status(&job_id, JobStatus::Skipped, None, Some(now_epoch()), None)
    {
        warn!("Failed to update skipped job status: {}", e);
    }

    // Mark all steps as skipped
    if let Ok(steps) = db.get_step_results_for_job(&job_id) {
        for step in &steps {
            let mut updated = step.clone();
            updated.status = StepStatus::Skipped;
            let _ = db.update_step_result(&updated);
        }
    }

    ctx.emit(AirlockEvent::JobCompleted {
        repo_id: run.repo_id.clone(),
        run_id: run.id.clone(),
        job_key: job_key.to_string(),
        status: "skipped".to_string(),
    });

    job_statuses.insert(job_key.to_string(), JobStatus::Skipped);
}

/// Mark a job as failed due to worktree acquisition failure.
pub(super) async fn fail_job_worktree(
    ctx: &Arc<HandlerContext>,
    run: &Run,
    job_key: &str,
    job_id_map: &HashMap<String, String>,
    job_statuses: &mut HashMap<String, JobStatus>,
    error_msg: &str,
) {
    error!("Job '{}' failed: {}", job_key, error_msg);

    if let Some(job_id) = job_id_map.get(job_key) {
        let db = ctx.db.lock().await;
        let _ = db.update_job_status(
            job_id,
            JobStatus::Failed,
            None,
            Some(now_epoch()),
            Some(error_msg),
        );
    }

    ctx.emit(AirlockEvent::JobCompleted {
        repo_id: run.repo_id.clone(),
        run_id: run.id.clone(),
        job_key: job_key.to_string(),
        status: "failed".to_string(),
    });

    job_statuses.insert(job_key.to_string(), JobStatus::Failed);
}

/// Create a log streaming callback that both emits events and writes to disk.
fn create_log_callback(
    ctx: &Arc<HandlerContext>,
    repo_id: &str,
    run_id: &str,
    job_key: &str,
    step_name: &str,
    logs_dir: &Path,
) -> LogStreamCallback {
    let log_repo_id = repo_id.to_string();
    let log_run_id = run_id.to_string();
    let log_job_key = job_key.to_string();
    let log_step_name = step_name.to_string();
    let log_ctx = ctx.clone();
    let log_logs_dir = logs_dir.to_path_buf();
    Arc::new(move |stream_type: &str, content: String| {
        log_ctx.emit(AirlockEvent::LogChunk {
            repo_id: log_repo_id.clone(),
            run_id: log_run_id.clone(),
            job_key: log_job_key.clone(),
            step_name: log_step_name.clone(),
            stream: stream_type.to_string(),
            content: content.clone(),
        });

        let filename = if stream_type == "stdout" {
            "stdout.log"
        } else {
            "stderr.log"
        };
        let path = log_logs_dir.join(filename);
        crate::pipeline::append_log_capped(&path, content.as_bytes());
    })
}

/// Parameters for executing a sequence of steps within a job.
pub(super) struct StepSequenceParams<'a> {
    pub ctx: &'a Arc<HandlerContext>,
    pub run: &'a Run,
    pub repo: &'a Repo,
    pub job_key: &'a str,
    pub job_config_name: Option<String>,
    pub worktree_path: &'a Path,
    pub effective_base_sha: &'a str,
    pub cancel: Option<&'a CancellationToken>,
    /// If set, clear the approval gate on the step at this order index (pre-paused re-execution).
    /// Uses step_order instead of name to avoid clearing approval on the wrong step
    /// when duplicate step names exist.
    pub clear_approval_for_step: Option<i32>,
    /// The step_order offset for the first step in `steps`.
    /// Used to match step results by `step_order` instead of name,
    /// avoiding corruption when duplicate step names exist.
    pub step_offset: usize,
}

/// Execute a sequence of steps, handling resolve/env/execute/emit for each.
///
/// Returns `(JobStatus, Option<String>)` — the final status and optional error message.
/// On failure, remaining Pending steps are marked as Skipped.
pub(super) async fn execute_step_sequence(
    params: &StepSequenceParams<'_>,
    steps: &[StepDefinition],
    step_results: &mut [StepResult],
) -> (JobStatus, Option<String>) {
    let stage_loader = StageLoader::default();
    let mut job_success = true;
    let mut job_error: Option<String> = None;
    let mut paused_for_approval = false;
    let mut effective_head_sha = params.run.head_sha.clone();

    for (i, step) in steps.iter().enumerate() {
        // Check cancellation before each step
        if let Some(cancel) = params.cancel {
            if cancel.is_cancelled() {
                info!(
                    "Run {} cancelled before step '{}' in job '{}'",
                    params.run.id, step.name, params.job_key
                );
                job_success = false;
                job_error = Some("Superseded by newer push".to_string());
                break;
            }
        }

        // Find the matching step result by step_order (not name) to handle
        // duplicate step names correctly.
        let expected_order = (params.step_offset + i) as i32;
        let step_result = match step_results
            .iter_mut()
            .find(|r| r.step_order == expected_order)
        {
            Some(r) => r,
            None => {
                error!(
                    "Step result with step_order={} not found in job '{}' (step '{}')",
                    expected_order, params.job_key, step.name
                );
                job_success = false;
                break;
            }
        };

        info!("Executing step '{}' in job '{}'", step.name, params.job_key);

        // Update current step in run
        {
            let db = params.ctx.db.lock().await;
            let _ = db.update_run_current_step(&params.run.id, Some(&step.name));
        }

        // Resolve reusable action
        let mut resolved_step = if step.is_reusable() {
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

        // If this is the pre-paused step being re-executed after approval,
        // clear the approval gate — the user already approved.
        // Match by step_order (not name) to only clear the originally approved step,
        // even when duplicate step names exist.
        if let Some(approved_order) = params.clear_approval_for_step {
            if expected_order == approved_order {
                resolved_step.require_approval = ApprovalMode::Never;
            }
        }

        // Build environment for this step
        let env_params = crate::pipeline::StageEnvironmentParams {
            paths: &params.ctx.paths,
            repo_id: &params.run.repo_id,
            run_id: &params.run.id,
            stage_name: &resolved_step.name,
            branch: &params.run.branch,
            base_sha: params.effective_base_sha,
            head_sha: &effective_head_sha,
            worktree_path: params.worktree_path,
            repo_root: &params.repo.working_path,
            upstream_url: &params.repo.upstream_url,
            gate_path: &params.repo.gate_path,
            job_key: Some(params.job_key),
        };

        let env = match crate::pipeline::build_stage_environment(&env_params) {
            Ok(mut e) => {
                e.job_name = params.job_config_name.clone();
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
            let db = params.ctx.db.lock().await;
            step_result.status = StepStatus::Running;
            step_result.started_at = Some(now_epoch());
            let _ = db.update_step_result(step_result);
        }

        // Emit StepStarted event
        params.ctx.emit(AirlockEvent::StepStarted {
            repo_id: params.run.repo_id.clone(),
            run_id: params.run.id.clone(),
            job_key: params.job_key.to_string(),
            step_name: step.name.clone(),
        });

        // Create log streaming callback
        let log_callback = create_log_callback(
            params.ctx,
            &params.run.repo_id,
            &params.run.id,
            params.job_key,
            &step.name,
            &env.logs_dir,
        );

        // Execute the step
        let timeout = std::time::Duration::from_secs(resolved_step.timeout.unwrap_or(60 * 60));
        let result = crate::pipeline::execute_stage_with_log_callback(
            &resolved_step,
            &step_result.id,
            &params.run.id,
            &env,
            timeout,
            Some(log_callback),
            params.cancel,
        )
        .await;

        match result {
            Ok(res) => {
                *step_result = res.clone();

                // Update step result in database
                {
                    let db = params.ctx.db.lock().await;
                    let _ = db.update_step_result(step_result);
                }

                // Check if the step produced a .head_sha artifact (e.g. from `airlock exec freeze`)
                if res.status == StepStatus::Passed || res.status == StepStatus::AwaitingApproval {
                    let head_sha_path = env.artifacts.join(".head_sha");
                    if head_sha_path.exists() {
                        if let Ok(contents) = std::fs::read_to_string(&head_sha_path) {
                            let new_sha = contents.trim().to_string();
                            if !new_sha.is_empty() && new_sha != effective_head_sha {
                                info!(
                                    "Step '{}' updated head_sha: {} -> {}",
                                    step.name,
                                    &effective_head_sha[..8.min(effective_head_sha.len())],
                                    &new_sha[..8.min(new_sha.len())]
                                );
                                effective_head_sha = new_sha.clone();
                                let db = params.ctx.db.lock().await;
                                let _ = db.update_run_head_sha(&params.run.id, &new_sha);
                            }
                        }
                        // Consume the artifact so subsequent steps don't re-read it
                        let _ = std::fs::remove_file(&head_sha_path);
                    }
                }

                // Auto-apply patches if apply-patch is set on the step
                if resolved_step.apply_patch && step_result.status == StepStatus::Passed {
                    match airlock_core::patches::apply_pending_patches(
                        params.worktree_path,
                        &env.artifacts,
                    ) {
                        Ok(Some(new_sha)) => {
                            info!(
                                "apply-patch '{}': HEAD -> {}",
                                step.name,
                                &new_sha[..8.min(new_sha.len())]
                            );
                            effective_head_sha = new_sha.clone();
                            let db = params.ctx.db.lock().await;
                            let _ = db.update_run_head_sha(&params.run.id, &new_sha);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            warn!("apply-patch '{}' failed: {}", step.name, e);
                            step_result.status = StepStatus::Failed;
                            step_result.error = Some(format!("apply-patch failed: {}", e));
                            let db = params.ctx.db.lock().await;
                            let _ = db.update_step_result(step_result);
                        }
                    }
                }

                // Emit StepCompleted event
                let status_str = step_status_str(step_result.status);
                params.ctx.emit(AirlockEvent::StepCompleted {
                    repo_id: params.run.repo_id.clone(),
                    run_id: params.run.id.clone(),
                    job_key: params.job_key.to_string(),
                    step_name: step.name.clone(),
                    status: status_str.to_string(),
                    branch: params.run.branch.clone(),
                });

                // Check if we should pause for approval
                if crate::pipeline::should_pause_for_approval(step_result) {
                    info!(
                        "Job '{}' paused at step '{}' awaiting approval",
                        params.job_key, step.name
                    );
                    paused_for_approval = true;
                    break;
                }

                // Check if we should continue
                if !crate::pipeline::should_continue_pipeline(&resolved_step, step_result) {
                    error!(
                        "Job '{}' stopped at step '{}' due to failure",
                        params.job_key, step.name
                    );
                    job_success = false;
                    job_error = step_result.error.clone();
                    break;
                }

                if step_result.status == StepStatus::Failed {
                    warn!(
                        "Step '{}' in job '{}' failed but continue_on_error=true, continuing",
                        step.name, params.job_key
                    );
                }
            }
            Err(e) => {
                error!(
                    "Step '{}' in job '{}' execution error: {}",
                    step.name, params.job_key, e
                );
                job_success = false;
                job_error = Some(e.to_string());

                // Update step as failed
                {
                    let db = params.ctx.db.lock().await;
                    step_result.status = StepStatus::Failed;
                    step_result.error = Some(e.to_string());
                    let _ = db.update_step_result(step_result);
                }

                params.ctx.emit(AirlockEvent::StepCompleted {
                    repo_id: params.run.repo_id.clone(),
                    run_id: params.run.id.clone(),
                    job_key: params.job_key.to_string(),
                    step_name: step.name.clone(),
                    status: "failed".to_string(),
                    branch: params.run.branch.clone(),
                });
                break;
            }
        }
    }

    // Mark any remaining Pending steps as Skipped
    if !job_success {
        let db = params.ctx.db.lock().await;
        for remaining in step_results.iter_mut() {
            if remaining.status == StepStatus::Pending {
                remaining.status = StepStatus::Skipped;
                let _ = db.update_step_result(remaining);
            }
        }
    }

    let final_status = if paused_for_approval {
        JobStatus::AwaitingApproval
    } else if job_success {
        JobStatus::Passed
    } else {
        JobStatus::Failed
    };

    (final_status, job_error)
}

/// Execute a single job: set up worktree, run steps sequentially.
///
/// Returns the final JobStatus. Checks `cancel` before each step.
#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_single_job(
    ctx: &Arc<HandlerContext>,
    run: &Run,
    repo: &Repo,
    job_key: &str,
    job_config: &JobConfig,
    job_id_map: &HashMap<String, String>,
    worktree_path: &Path,
    cancel: &CancellationToken,
) -> JobStatus {
    let job_id = match job_id_map.get(job_key) {
        Some(id) => id.clone(),
        None => {
            error!("Job ID not found for key '{}'", job_key);
            return JobStatus::Failed;
        }
    };

    info!(
        "Starting job '{}' ({}) with {} step(s)",
        job_key,
        job_config.name.as_deref().unwrap_or("unnamed"),
        job_config.steps.len()
    );

    // Mark job as running
    {
        let db = ctx.db.lock().await;
        if let Err(e) =
            db.update_job_status(&job_id, JobStatus::Running, Some(now_epoch()), None, None)
        {
            warn!("Failed to update job status to running: {}", e);
        }
    }

    ctx.emit(AirlockEvent::JobStarted {
        repo_id: run.repo_id.clone(),
        run_id: run.id.clone(),
        job_key: job_key.to_string(),
    });

    // Ensure worktree exists
    if !worktree_path.exists() {
        if let Err(e) =
            airlock_core::create_run_worktree(&repo.gate_path, worktree_path, &run.head_sha)
        {
            error!("Failed to create worktree for job '{}': {}", job_key, e);
            let db = ctx.db.lock().await;
            let _ = db.update_job_status(
                &job_id,
                JobStatus::Failed,
                None,
                Some(now_epoch()),
                Some(&format!("Failed to create worktree: {}", e)),
            );
            ctx.emit(AirlockEvent::JobCompleted {
                repo_id: run.repo_id.clone(),
                run_id: run.id.clone(),
                job_key: job_key.to_string(),
                status: "failed".to_string(),
            });
            return JobStatus::Failed;
        }
    }

    // Resolve effective base SHA
    let effective_base_sha =
        crate::pipeline::resolve_effective_base_sha(worktree_path, &run.base_sha).unwrap_or_else(
            |e| {
                warn!(
                    "Failed to resolve effective base SHA: {}, using original",
                    e
                );
                run.base_sha.clone()
            },
        );

    // Get step results from DB (for updating)
    let mut step_results = {
        let db = ctx.db.lock().await;
        match db.get_step_results_for_job(&job_id) {
            Ok(r) => r,
            Err(e) => {
                error!("Failed to get step results for job '{}': {}", job_key, e);
                let _ = db.update_job_status(
                    &job_id,
                    JobStatus::Failed,
                    None,
                    Some(now_epoch()),
                    Some(&format!("Failed to get step results: {}", e)),
                );
                return JobStatus::Failed;
            }
        }
    };

    // Execute steps
    let seq_params = StepSequenceParams {
        ctx,
        run,
        repo,
        job_key,
        job_config_name: job_config.name.clone(),
        worktree_path,
        effective_base_sha: &effective_base_sha,
        cancel: Some(cancel),
        clear_approval_for_step: None,
        step_offset: 0,
    };

    let (final_status, job_error) =
        execute_step_sequence(&seq_params, &job_config.steps, &mut step_results).await;

    // Update job status
    {
        let db = ctx.db.lock().await;
        let completed_at = if final_status == JobStatus::AwaitingApproval {
            None
        } else {
            Some(now_epoch())
        };
        if let Err(e) = db.update_job_status(
            &job_id,
            final_status,
            None, // started_at already set
            completed_at,
            job_error.as_deref(),
        ) {
            warn!("Failed to update job status: {}", e);
        }

        // Clear current step if not paused
        if final_status != JobStatus::AwaitingApproval {
            let _ = db.update_run_current_step(&run.id, None);
        }
    }

    let status_str = match final_status {
        JobStatus::Passed => "passed",
        JobStatus::Failed => "failed",
        JobStatus::Skipped => "skipped",
        JobStatus::AwaitingApproval => "awaiting_approval",
        JobStatus::Running => "running",
        JobStatus::Pending => "pending",
    };

    ctx.emit(AirlockEvent::JobCompleted {
        repo_id: run.repo_id.clone(),
        run_id: run.id.clone(),
        job_key: job_key.to_string(),
        status: status_str.to_string(),
    });

    info!("Job '{}' completed with status: {}", job_key, status_str);
    final_status
}

/// Resume DAG execution after a job completes (e.g., after approval resumes a paused job).
///
/// This checks if any dependent jobs in the workflow are now unblocked and executes them.
/// It continues until no more jobs can be started. This handles the case where approving
/// a step causes a job to complete, which in turn unblocks downstream jobs.
///
/// `completed_job_key` is the job that just finished.
/// `completed_job_status` is its final status.
///
/// Note: This path is not cancellable — approvals are user-initiated and should
/// run to completion.
pub(super) async fn resume_dag_after_job_completion(
    ctx: &Arc<HandlerContext>,
    run: &Run,
    repo: &Repo,
    workflow: &WorkflowConfig,
    completed_job_key: &str,
    completed_job_status: JobStatus,
) {
    // Reload run from DB to get the latest head_sha. Between when the caller
    // loaded the run and now, apply_patches may have updated head_sha.
    let run = {
        let db = ctx.db.lock().await;
        match db.get_run(&run.id) {
            Ok(Some(r)) => r,
            Ok(None) => {
                error!("Run {} no longer exists in DB during DAG resume", run.id);
                return;
            }
            Err(e) => {
                error!("Failed to reload run {} during DAG resume: {}", run.id, e);
                return;
            }
        }
    };
    let run = &run;

    // Build current job statuses, worktree paths from DB
    let mut job_statuses: HashMap<String, JobStatus> = HashMap::new();
    let mut job_id_map: HashMap<String, String> = HashMap::new();
    let mut job_worktrees: HashMap<String, PathBuf> = HashMap::new();
    let mut job_leases: HashMap<String, PoolLease> = HashMap::new();

    {
        let db = ctx.db.lock().await;
        match db.get_job_results_for_run(&run.id) {
            Ok(jobs) => {
                for job in &jobs {
                    job_statuses.insert(job.job_key.clone(), job.status);
                    job_id_map.insert(job.job_key.clone(), job.id.clone());
                    if let Some(ref wt_path) = job.worktree_path {
                        job_worktrees.insert(job.job_key.clone(), PathBuf::from(wt_path));
                    }
                }
            }
            Err(e) => {
                error!("Failed to get job results for DAG resume: {}", e);
                return;
            }
        }
    }

    // Make sure the completed job's status is up-to-date in our map
    job_statuses.insert(completed_job_key.to_string(), completed_job_status);

    // Find jobs that are still Pending and check if their dependencies are now all satisfied
    let mut newly_runnable: Vec<String> = Vec::new();
    // Approval-resumed jobs are not cancellable (user explicitly approved them)
    let no_cancel = CancellationToken::new();

    loop {
        newly_runnable.clear();

        for (job_key, job_config) in workflow.jobs.iter() {
            // Only consider Pending jobs
            if job_statuses.get(job_key.as_str()) != Some(&JobStatus::Pending) {
                continue;
            }

            // Check if all dependencies are satisfied (i.e., in a final state)
            let all_deps_done = job_config.needs.iter().all(|dep| {
                job_statuses
                    .get(dep.as_str())
                    .map(|s| s.is_final())
                    .unwrap_or(false)
            });

            if !all_deps_done {
                continue;
            }

            // Check if any dep failed (should skip this job)
            if should_skip_job(job_key, job_config, &job_statuses) {
                skip_job(ctx, run, job_key, &job_id_map, &mut job_statuses).await;
                // Re-check newly unblocked jobs in next iteration
                newly_runnable.clear();
                continue;
            }

            newly_runnable.push(job_key.clone());
        }

        if newly_runnable.is_empty() {
            break;
        }

        // Execute newly runnable jobs in parallel
        if newly_runnable.len() == 1 {
            let job_key = &newly_runnable[0];
            let job_config = workflow.jobs.get(job_key).unwrap();

            let (worktree_path, lease) =
                match resolve_job_worktree(ctx, job_key, run, repo, &job_id_map).await {
                    Ok(result) => result,
                    Err(e) => {
                        fail_job_worktree(ctx, run, job_key, &job_id_map, &mut job_statuses, &e)
                            .await;
                        continue;
                    }
                };

            let status = execute_single_job(
                ctx,
                run,
                repo,
                job_key,
                job_config,
                &job_id_map,
                &worktree_path,
                &no_cancel,
            )
            .await;

            job_worktrees.insert(job_key.clone(), worktree_path);
            if let Some(l) = lease {
                job_leases.insert(job_key.clone(), l);
            }
            job_statuses.insert(job_key.clone(), status);
        } else {
            // Multiple jobs — acquire worktrees before spawning
            let mut wave_jobs: Vec<(String, PathBuf, Option<PoolLease>)> = Vec::new();

            for job_key in &newly_runnable {
                match resolve_job_worktree(ctx, job_key, run, repo, &job_id_map).await {
                    Ok((worktree_path, lease)) => {
                        wave_jobs.push((job_key.clone(), worktree_path, lease));
                    }
                    Err(e) => {
                        fail_job_worktree(ctx, run, job_key, &job_id_map, &mut job_statuses, &e)
                            .await;
                    }
                }
            }

            let spawned_keys: Vec<String> = wave_jobs.iter().map(|(k, _, _)| k.clone()).collect();

            let mut join_set = tokio::task::JoinSet::new();
            for (job_key, worktree_path, lease) in wave_jobs {
                if let Some(l) = lease {
                    job_leases.insert(job_key.clone(), l);
                }
                job_worktrees.insert(job_key.clone(), worktree_path.clone());

                let ctx = ctx.clone();
                let run = run.clone();
                let repo = repo.clone();
                let job_config = workflow.jobs.get(&job_key).unwrap().clone();
                let job_id_map = job_id_map.clone();
                let no_cancel = no_cancel.clone();

                join_set.spawn(async move {
                    let status = execute_single_job(
                        &ctx,
                        &run,
                        &repo,
                        &job_key,
                        &job_config,
                        &job_id_map,
                        &worktree_path,
                        &no_cancel,
                    )
                    .await;
                    (job_key, status)
                });
            }

            while let Some(result) = join_set.join_next().await {
                match result {
                    Ok((job_key, status)) => {
                        job_statuses.insert(job_key, status);
                    }
                    Err(e) => {
                        error!("Job task panicked during DAG resume: {}", e);
                    }
                }
            }

            // Mark any spawned jobs that didn't return a result (panicked) as Failed
            for key in &spawned_keys {
                if !job_statuses.contains_key(key) {
                    error!(
                        "Job '{}' panicked during DAG resume — marking as failed",
                        key
                    );
                    job_statuses.insert(key.clone(), JobStatus::Failed);
                    if let Some(job_id) = job_id_map.get(key) {
                        let db = ctx.db.lock().await;
                        let _ = db.update_job_status(
                            job_id,
                            JobStatus::Failed,
                            None,
                            Some(now_epoch()),
                            Some("Internal error: job task panicked"),
                        );
                    }
                }
            }
        }

        // Loop again to check if more jobs got unblocked
    }

    release_leases_and_cleanup(ctx, run, repo, &job_leases, &job_worktrees, &job_statuses).await;
}

/// Release pool leases and clean up ephemeral worktrees after jobs finish.
///
/// Pool leases are held (not released) if any job using that worktree slot is
/// still paused (`AwaitingApproval`). Ephemeral worktrees are removed unless
/// they belong to a paused job or are tracked by the pool.
async fn release_leases_and_cleanup(
    ctx: &Arc<HandlerContext>,
    run: &Run,
    repo: &Repo,
    job_leases: &HashMap<String, PoolLease>,
    job_worktrees: &HashMap<String, PathBuf>,
    job_statuses: &HashMap<String, JobStatus>,
) {
    // Release pool leases — but only if no job using that worktree is paused
    for lease in job_leases.values() {
        let lease_path = &lease.path;
        let any_holds = job_worktrees.iter().any(|(jk, wt)| {
            wt == lease_path && job_statuses.get(jk) == Some(&JobStatus::AwaitingApproval)
        });

        if !any_holds {
            ctx.worktree_pool
                .release(&run.repo_id, lease.slot_index)
                .await;
        }
    }

    // Clean up ephemeral worktrees (those without pool leases and not pool-managed)
    let leased_paths: std::collections::HashSet<&PathBuf> =
        job_leases.values().map(|l| &l.path).collect();
    for (jk, wt) in job_worktrees {
        if leased_paths.contains(wt) {
            continue; // managed by pool (lease acquired in this call)
        }
        if job_statuses.get(jk) == Some(&JobStatus::AwaitingApproval) {
            continue; // still in use
        }
        // Check if the pool tracks this worktree (e.g. from a previously completed job).
        // Pool worktrees must not be deleted — they're reused across runs.
        if ctx
            .worktree_pool
            .find_lease_by_path(&run.repo_id, wt)
            .await
            .is_some()
        {
            continue; // pool-managed worktree from a prior job
        }
        if let Err(e) = airlock_core::remove_run_worktree(&repo.gate_path, wt) {
            warn!("Failed to clean up ephemeral worktree {:?}: {}", wt, e);
        }
    }
}

/// Convert StepStatus to a string for IPC events.
pub(super) fn step_status_str(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Passed => "passed",
        StepStatus::Failed => "failed",
        StepStatus::Skipped => "skipped",
        StepStatus::AwaitingApproval => "awaiting_approval",
        StepStatus::Running => "running",
        StepStatus::Pending => "pending",
    }
}

/// Mark a run as cancelled (superseded by a newer push or stopped by user).
///
/// If the run already has an error set (e.g. "Stopped by user"), it is
/// preserved.  Otherwise we default to "Superseded by newer push".
async fn mark_run_cancelled(ctx: &Arc<HandlerContext>, run: &Run) {
    let db = ctx.db.lock().await;
    // Only set the default cancellation message if no error is already present
    let existing = db.get_run(&run.id).ok().flatten();
    let already_has_error = existing.as_ref().is_some_and(|r| r.error.is_some());
    if !already_has_error {
        let _ = db.update_run_error(&run.id, Some("Superseded by newer push"));
    }
    drop(db);

    ctx.emit(AirlockEvent::RunCompleted {
        repo_id: run.repo_id.clone(),
        run_id: run.id.clone(),
        success: false,
        branch: run.branch.clone(),
    });
}

/// Check the state of all jobs and emit appropriate run-level events.
pub(super) async fn emit_run_final_status(ctx: &Arc<HandlerContext>, run: &Run) {
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
            info!("Pipeline completed successfully for run {}", run.id);
            ctx.emit(AirlockEvent::RunCompleted {
                repo_id: run.repo_id.clone(),
                run_id: run.id.clone(),
                success: true,
                branch: run.branch.clone(),
            });
        } else if any_failed {
            error!("Pipeline failed for run {}", run.id);
            let db = ctx.db.lock().await;
            let _ = db.update_run_error(&run.id, Some("One or more jobs failed"));
            drop(db);

            ctx.emit(AirlockEvent::RunCompleted {
                repo_id: run.repo_id.clone(),
                run_id: run.id.clone(),
                success: false,
                branch: run.branch.clone(),
            });
        } else {
            // All done but not all passed and none failed — e.g., all skipped
            ctx.emit(AirlockEvent::RunCompleted {
                repo_id: run.repo_id.clone(),
                run_id: run.id.clone(),
                success: false,
                branch: run.branch.clone(),
            });
        }
    }
    // If not all done and none paused, there's likely still running jobs — don't emit
}

/// Get current epoch time in seconds.
pub(super) fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use airlock_core::{Database, RefUpdate};

    #[test]
    fn test_extract_branch_name_simple() {
        let updates = vec![RefUpdate {
            ref_name: "refs/heads/main".to_string(),
            old_sha: "abc123".to_string(),
            new_sha: "def456".to_string(),
        }];

        assert_eq!(extract_branch_name(&updates), Some("main".to_string()));
    }

    #[test]
    fn test_extract_branch_name_feature_branch() {
        let updates = vec![RefUpdate {
            ref_name: "refs/heads/feature/add-auth".to_string(),
            old_sha: "abc123".to_string(),
            new_sha: "def456".to_string(),
        }];

        assert_eq!(
            extract_branch_name(&updates),
            Some("feature/add-auth".to_string())
        );
    }

    #[test]
    fn test_extract_branch_name_skips_deletion() {
        let updates = vec![
            RefUpdate {
                ref_name: "refs/heads/old-branch".to_string(),
                old_sha: "abc123".to_string(),
                new_sha: "0000000000000000000000000000000000000000".to_string(), // deletion
            },
            RefUpdate {
                ref_name: "refs/heads/new-branch".to_string(),
                old_sha: "000000".to_string(),
                new_sha: "def456".to_string(),
            },
        ];

        assert_eq!(
            extract_branch_name(&updates),
            Some("new-branch".to_string())
        );
    }

    #[test]
    fn test_extract_branch_name_no_branches() {
        let updates: Vec<RefUpdate> = vec![];
        assert_eq!(extract_branch_name(&updates), None);
    }

    #[test]
    fn test_extract_branch_name_only_deletions() {
        let updates = vec![RefUpdate {
            ref_name: "refs/heads/deleted".to_string(),
            old_sha: "abc123".to_string(),
            new_sha: "0000000000000000000000000000000000000000".to_string(),
        }];

        assert_eq!(extract_branch_name(&updates), None);
    }

    #[test]
    fn test_extract_branch_name_ignores_tags() {
        let updates = vec![RefUpdate {
            ref_name: "refs/tags/v1.0.0".to_string(),
            old_sha: "abc123".to_string(),
            new_sha: "def456".to_string(),
        }];

        assert_eq!(extract_branch_name(&updates), None);
    }

    #[tokio::test]
    async fn test_execute_step_sequence_reads_head_sha_artifact() {
        use crate::handlers::HandlerContext;
        use airlock_core::{
            AirlockPaths, ApprovalMode, JobResult, JobStatus as JS, Repo, Run, StepDefinition,
            StepResult, StepStatus,
        };
        use tokio::sync::watch;

        let tmp = tempfile::tempdir().unwrap();
        let paths = AirlockPaths::with_root(tmp.path().to_path_buf());
        let db = Database::open_in_memory().unwrap();
        let (shutdown_tx, _) = watch::channel(false);

        let repo_id = "repo-head-sha";
        let run_id = "run-head-sha";
        let job_id = "job-head-sha";
        let original_sha = "aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111";
        let new_sha = "bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222";

        // Create a worktree directory (just a temp dir, step doesn't use git)
        let worktree_path = tmp.path().join("worktree");
        std::fs::create_dir_all(&worktree_path).unwrap();

        // Create a repo directory
        let repo_root = tmp.path().join("repo");
        std::fs::create_dir_all(&repo_root).unwrap();
        let gate_path = tmp.path().join("gate.git");
        std::fs::create_dir_all(&gate_path).unwrap();

        let repo = Repo {
            id: repo_id.to_string(),
            working_path: repo_root.clone(),
            upstream_url: "git@github.com:user/repo.git".to_string(),
            gate_path: gate_path.clone(),
            last_sync: None,
            created_at: 1704067200,
        };
        db.insert_repo(&repo).unwrap();

        // Insert run into DB
        let run = Run {
            id: run_id.to_string(),
            repo_id: repo_id.to_string(),
            ref_updates: vec![RefUpdate {
                ref_name: "refs/heads/main".to_string(),
                old_sha: "0000000000000000000000000000000000000000".to_string(),
                new_sha: original_sha.to_string(),
            }],
            error: None,
            superseded: false,
            created_at: 1704067200,
            branch: "refs/heads/main".to_string(),
            base_sha: "0000000000000000000000000000000000000000".to_string(),
            head_sha: original_sha.to_string(),
            current_step: None,
            updated_at: 1704067200,
            workflow_file: "main.yml".to_string(),
            workflow_name: None,
        };
        db.insert_run(&run).unwrap();

        // Insert job result (FK for step_results)
        let job_result = JobResult {
            id: job_id.to_string(),
            run_id: run_id.to_string(),
            job_key: "build".to_string(),
            name: None,
            status: JS::Running,
            job_order: 0,
            started_at: Some(1704067200),
            completed_at: None,
            error: None,
            worktree_path: None,
        };
        db.insert_job_result(&job_result).unwrap();

        // Pre-create the artifacts dir since build_stage_environment will use it
        let artifacts_dir = paths.run_artifacts(repo_id, run_id);
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        // Step: write new SHA to .head_sha in artifacts dir
        let step = StepDefinition {
            name: "update-head".to_string(),
            run: Some(format!(
                "printf '%s' '{}' > \"$AIRLOCK_ARTIFACTS/.head_sha\"",
                new_sha
            )),
            uses: None,
            shell: Some("sh".to_string()),
            env: Default::default(),
            continue_on_error: false,
            require_approval: ApprovalMode::Never,
            timeout: Some(10),
            apply_patch: false,
        };

        let step_result_id = "step-head-sha";
        let mut step_results = vec![StepResult {
            id: step_result_id.to_string(),
            run_id: run_id.to_string(),
            job_id: job_id.to_string(),
            name: "update-head".to_string(),
            status: StepStatus::Pending,
            step_order: 0,
            exit_code: None,
            duration_ms: None,
            error: None,
            started_at: None,
            completed_at: None,
        }];
        db.insert_step_result(&step_results[0]).unwrap();

        let ctx = Arc::new(HandlerContext::new(paths, db, shutdown_tx));

        let params = StepSequenceParams {
            ctx: &ctx,
            run: &run,
            repo: &repo,
            job_key: "build",
            job_config_name: None,
            worktree_path: &worktree_path,
            effective_base_sha: &run.base_sha,
            cancel: None,
            clear_approval_for_step: None,
            step_offset: 0,
        };

        let (status, error) = execute_step_sequence(&params, &[step], &mut step_results).await;

        assert_eq!(status, JobStatus::Passed, "Job should pass: {:?}", error);

        // Verify DB was updated with new head_sha
        let db = ctx.db.lock().await;
        let updated_run = db.get_run(run_id).unwrap().unwrap();
        assert_eq!(
            updated_run.head_sha, new_sha,
            "DB head_sha should be updated to the new SHA written by the step"
        );

        // Verify .head_sha artifact was consumed (deleted)
        assert!(
            !artifacts_dir.join(".head_sha").exists(),
            ".head_sha artifact should be consumed after reading"
        );
    }
}
