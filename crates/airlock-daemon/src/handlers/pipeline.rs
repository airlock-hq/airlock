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
use airlock_core::{
    filter_workflows_for_branch, load_workflows_from_tree, validate_job_dag, JobConfig, JobResult,
    JobStatus, RefUpdate, Repo, Run, StepResult, StepStatus, WorkflowConfig,
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
async fn execute_workflow_dag(
    ctx: &Arc<HandlerContext>,
    run: &Run,
    repo: &Repo,
    workflow: &WorkflowConfig,
    waves: &[Vec<String>],
    job_id_map: &HashMap<String, String>,
    cancel: &CancellationToken,
) {
    let paths = ctx.paths.clone();

    // Create run artifacts directory
    if let Err(e) = crate::pipeline::create_run_artifacts_dir(&paths, &run.repo_id, &run.id) {
        warn!("Failed to create run artifacts directory: {}", e);
    }

    // Track job statuses and worktree paths for inheritance
    let mut job_statuses: HashMap<String, JobStatus> = HashMap::new();
    let mut job_worktrees: HashMap<String, PathBuf> = HashMap::new();
    let mut workflow_failed = false;

    for (wave_idx, wave) in waves.iter().enumerate() {
        // Check cancellation before each wave
        if cancel.is_cancelled() {
            info!("Run {} cancelled before wave {}", run.id, wave_idx + 1);
            // Mark all remaining jobs as skipped
            for remaining_wave in &waves[wave_idx..] {
                for jk in remaining_wave {
                    skip_job(ctx, run, jk, job_id_map, &mut job_statuses).await;
                }
            }
            mark_run_cancelled(ctx, run).await;
            return;
        }

        debug!(
            "Executing wave {}/{}: {:?}",
            wave_idx + 1,
            waves.len(),
            wave
        );

        if wave.len() == 1 {
            // Single job in wave — execute directly (no spawn overhead)
            let job_key = &wave[0];
            let job_config = workflow.jobs.get(job_key).unwrap();

            // Check if this job should be skipped (dependency failed)
            if should_skip_job(job_key, job_config, &job_statuses) {
                skip_job(ctx, run, job_key, job_id_map, &mut job_statuses).await;
                workflow_failed = true;
                continue;
            }

            let worktree_path =
                resolve_job_worktree(job_key, job_config, &job_worktrees, &paths, run, repo);

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

            job_worktrees.insert(job_key.clone(), worktree_path.clone());

            if status == JobStatus::Failed {
                workflow_failed = true;
            }
            job_statuses.insert(job_key.clone(), status);
        } else {
            // Multiple jobs in wave — execute in parallel using tokio::JoinSet
            let mut join_set = tokio::task::JoinSet::new();

            // Collect job details before spawning
            let mut wave_jobs: Vec<(String, PathBuf)> = Vec::new();
            for job_key in wave {
                let job_config = workflow.jobs.get(job_key).unwrap();

                if should_skip_job(job_key, job_config, &job_statuses) {
                    skip_job(ctx, run, job_key, job_id_map, &mut job_statuses).await;
                    workflow_failed = true;
                    continue;
                }

                let worktree_path =
                    resolve_job_worktree(job_key, job_config, &job_worktrees, &paths, run, repo);

                wave_jobs.push((job_key.clone(), worktree_path));
            }

            for (job_key, worktree_path) in wave_jobs {
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
                    (job_key, worktree_path, status)
                });
            }

            // Collect results
            while let Some(result) = join_set.join_next().await {
                match result {
                    Ok((job_key, worktree_path, status)) => {
                        job_worktrees.insert(job_key.clone(), worktree_path);
                        if status == JobStatus::Failed {
                            workflow_failed = true;
                        }
                        job_statuses.insert(job_key, status);
                    }
                    Err(e) => {
                        error!("Job task panicked: {}", e);
                        workflow_failed = true;
                    }
                }
            }
        }
    }

    // Clean up ephemeral worktrees only. The persistent per-repo worktree
    // is kept alive so build caches survive across runs.
    let persistent_wt = paths.repo_worktree(&run.repo_id);
    let any_paused = job_statuses
        .values()
        .any(|s| *s == JobStatus::AwaitingApproval);
    for (job_key, worktree_path) in &job_worktrees {
        // Never remove the persistent worktree
        if *worktree_path == persistent_wt {
            continue;
        }
        let job_config = workflow.jobs.get(job_key);
        let keep = job_config.map(|c| c.keep_worktrees).unwrap_or(false);
        let is_paused = job_statuses.get(job_key) == Some(&JobStatus::AwaitingApproval);

        if !keep && !is_paused && worktree_path.exists() {
            if let Err(e) = airlock_core::remove_worktree(&repo.gate_path, worktree_path) {
                warn!("Failed to remove worktree for job '{}': {}", job_key, e);
            }
        }
    }

    // Emit final run result
    if any_paused {
        info!(
            "Workflow paused for run {} (one or more jobs awaiting approval)",
            run.id
        );
        ctx.emit(AirlockEvent::RunUpdated {
            repo_id: run.repo_id.clone(),
            run_id: run.id.clone(),
            status: "awaiting_approval".to_string(),
        });
    } else if workflow_failed {
        error!("Workflow failed for run {}", run.id);
        ctx.emit(AirlockEvent::RunCompleted {
            repo_id: run.repo_id.clone(),
            run_id: run.id.clone(),
            success: false,
        });
    } else {
        info!("Workflow completed successfully for run {}", run.id);
        ctx.emit(AirlockEvent::RunCompleted {
            repo_id: run.repo_id.clone(),
            run_id: run.id.clone(),
            success: true,
        });
    }
}

/// Determine the worktree path for a job based on inheritance rules.
///
/// - The first/only job (no predecessors, empty worktree map) uses the
///   persistent per-repo worktree so build caches survive across runs.
/// - Jobs with exactly one predecessor inherit that predecessor's worktree.
/// - Jobs with multiple predecessors get a fresh ephemeral worktree.
pub(super) fn resolve_job_worktree(
    job_key: &str,
    job_config: &JobConfig,
    job_worktrees: &HashMap<String, PathBuf>,
    paths: &airlock_core::AirlockPaths,
    run: &Run,
    repo: &Repo,
) -> PathBuf {
    // Single predecessor → inherit worktree
    if job_config.needs.len() == 1 {
        let predecessor = &job_config.needs[0];
        if let Some(wt) = job_worktrees.get(predecessor.as_str()) {
            debug!(
                "Job '{}' inherits worktree from predecessor '{}'",
                job_key, predecessor
            );
            return wt.clone();
        }
    }

    // No predecessors or multiple predecessors → fresh worktree
    // For the first/only job, use the persistent per-repo worktree
    if job_config.needs.is_empty() && job_worktrees.is_empty() {
        let wt = paths.repo_worktree(&run.repo_id);
        if let Err(e) = airlock_core::reset_persistent_worktree(&repo.gate_path, &wt, &run.head_sha)
        {
            error!(
                "Failed to reset persistent worktree for job '{}': {}, falling back to ephemeral worktree",
                job_key, e
            );
            // Fall back to an ephemeral worktree instead of returning a broken path
            let ephemeral_wt = paths.run_worktree(&run.repo_id, &run.id);
            if let Err(e2) =
                airlock_core::create_run_worktree(&repo.gate_path, &ephemeral_wt, &run.head_sha)
            {
                error!(
                    "Ephemeral worktree fallback also failed for job '{}': {}",
                    job_key, e2
                );
            }
            return ephemeral_wt;
        }
        return wt;
    }

    // Multiple predecessors or non-first job without inheritance → ephemeral worktree
    let wt = paths
        .run_worktree(&run.repo_id, &run.id)
        .with_extension(job_key);
    if !wt.exists() {
        if let Err(e) = airlock_core::create_run_worktree(&repo.gate_path, &wt, &run.head_sha) {
            error!("Failed to create worktree for job '{}': {}", job_key, e);
        }
    }
    wt
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

    // Create stage loader
    let stage_loader = StageLoader::default();

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

    // Execute steps sequentially
    let mut job_success = true;
    let mut job_error: Option<String> = None;
    let mut paused_for_approval = false;

    for (idx, step) in job_config.steps.iter().enumerate() {
        // Check cancellation before each step
        if cancel.is_cancelled() {
            info!(
                "Run {} cancelled before step '{}' in job '{}'",
                run.id, step.name, job_key
            );
            job_success = false;
            job_error = Some("Superseded by newer push".to_string());
            break;
        }

        // Find the matching step result (by order)
        let step_result = match step_results.get_mut(idx) {
            Some(r) => r,
            None => {
                error!(
                    "Step result at index {} not found for job '{}'",
                    idx, job_key
                );
                job_success = false;
                break;
            }
        };

        info!(
            "Executing step {}/{} in job '{}': {}",
            idx + 1,
            job_config.steps.len(),
            job_key,
            step.name
        );

        // Update current step in run
        {
            let db = ctx.db.lock().await;
            let _ = db.update_run_current_step(&run.id, Some(&step.name));
        }

        // Resolve reusable action
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
            base_sha: &effective_base_sha,
            head_sha: &run.head_sha,
            worktree_path,
            repo_root: &repo.working_path,
            upstream_url: &repo.upstream_url,
            gate_path: &repo.gate_path,
            job_key: Some(job_key),
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
            job_key: job_key.to_string(),
            step_name: step.name.clone(),
        });

        // Create log streaming callback that both emits events and writes to disk
        let log_repo_id = run.repo_id.clone();
        let log_run_id = run.id.clone();
        let log_job_key = job_key.to_string();
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
            Some(cancel),
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
                let status_str = step_status_str(res.status);
                ctx.emit(AirlockEvent::StepCompleted {
                    repo_id: run.repo_id.clone(),
                    run_id: run.id.clone(),
                    job_key: job_key.to_string(),
                    step_name: step.name.clone(),
                    status: status_str.to_string(),
                });

                // Check if we should pause for approval
                if crate::pipeline::should_pause_for_approval(&res) {
                    info!(
                        "Job '{}' paused at step '{}' awaiting approval",
                        job_key, step.name
                    );
                    paused_for_approval = true;
                    break;
                }

                // Check if we should continue
                if !crate::pipeline::should_continue_pipeline(&resolved_step, &res) {
                    error!(
                        "Job '{}' stopped at step '{}' due to failure",
                        job_key, step.name
                    );
                    job_success = false;
                    job_error = res.error.clone();
                    break;
                }

                if res.status == StepStatus::Failed {
                    warn!(
                        "Step '{}' in job '{}' failed but continue_on_error=true, continuing",
                        step.name, job_key
                    );
                }
            }
            Err(e) => {
                error!(
                    "Step '{}' in job '{}' execution error: {}",
                    step.name, job_key, e
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

                ctx.emit(AirlockEvent::StepCompleted {
                    repo_id: run.repo_id.clone(),
                    run_id: run.id.clone(),
                    job_key: job_key.to_string(),
                    step_name: step.name.clone(),
                    status: "failed".to_string(),
                });
                break;
            }
        }
    }

    // Mark any remaining Pending steps as Skipped.
    // This handles all early-break paths (execution errors, resolve failures,
    // environment build failures) where remaining steps weren't cleaned up.
    if !job_success {
        let db = ctx.db.lock().await;
        for remaining in step_results.iter_mut() {
            if remaining.status == StepStatus::Pending {
                remaining.status = StepStatus::Skipped;
                let _ = db.update_step_result(remaining);
            }
        }
    }

    // Determine final job status
    let final_status = if paused_for_approval {
        JobStatus::AwaitingApproval
    } else if job_success {
        JobStatus::Passed
    } else {
        JobStatus::Failed
    };

    // Update job status
    {
        let db = ctx.db.lock().await;
        let completed_at = if paused_for_approval {
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
        if !paused_for_approval {
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
    let paths = ctx.paths.clone();

    // Build current job statuses from DB
    let mut job_statuses: HashMap<String, JobStatus> = HashMap::new();
    let mut job_id_map: HashMap<String, String> = HashMap::new();
    let mut job_worktrees: HashMap<String, PathBuf> = HashMap::new();

    {
        let db = ctx.db.lock().await;
        match db.get_job_results_for_run(&run.id) {
            Ok(jobs) => {
                for job in &jobs {
                    job_statuses.insert(job.job_key.clone(), job.status);
                    job_id_map.insert(job.job_key.clone(), job.id.clone());
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

    // Populate worktree paths for completed jobs (they may exist on disk)
    let persistent_wt_path = paths.repo_worktree(&run.repo_id);
    for job_key in job_statuses.keys() {
        // Check persistent worktree first (new default)
        if persistent_wt_path.exists() {
            job_worktrees.insert(job_key.clone(), persistent_wt_path.clone());
        }
        let wt = paths.run_worktree(&run.repo_id, &run.id);
        if wt.exists() {
            job_worktrees.insert(job_key.clone(), wt.clone());
        }
        let job_wt = wt.with_extension(job_key.as_str());
        if job_wt.exists() {
            job_worktrees.insert(job_key.clone(), job_wt);
        }
    }

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

            let worktree_path =
                resolve_job_worktree(job_key, job_config, &job_worktrees, &paths, run, repo);

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
            job_statuses.insert(job_key.clone(), status);
        } else {
            // Multiple jobs — execute in parallel
            let mut join_set = tokio::task::JoinSet::new();

            for job_key in &newly_runnable {
                let job_config = workflow.jobs.get(job_key).unwrap();
                let worktree_path =
                    resolve_job_worktree(job_key, job_config, &job_worktrees, &paths, run, repo);

                let ctx = ctx.clone();
                let run = run.clone();
                let repo = repo.clone();
                let job_config = job_config.clone();
                let job_id_map = job_id_map.clone();
                let job_key = job_key.clone();
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
                    (job_key, worktree_path, status)
                });
            }

            while let Some(result) = join_set.join_next().await {
                match result {
                    Ok((job_key, worktree_path, status)) => {
                        job_worktrees.insert(job_key.clone(), worktree_path);
                        job_statuses.insert(job_key, status);
                    }
                    Err(e) => {
                        error!("Job task panicked during DAG resume: {}", e);
                    }
                }
            }
        }

        // Loop again to check if more jobs got unblocked
    }

    // Clean up ephemeral worktrees for newly completed jobs.
    // The persistent per-repo worktree is never removed.
    let persistent_wt = paths.repo_worktree(&run.repo_id);
    for (job_key, worktree_path) in &job_worktrees {
        if *worktree_path == persistent_wt {
            continue;
        }
        let job_config = workflow.jobs.get(job_key);
        let keep = job_config.map(|c| c.keep_worktrees).unwrap_or(false);
        let is_paused = job_statuses.get(job_key) == Some(&JobStatus::AwaitingApproval);

        if !keep && !is_paused && worktree_path.exists() {
            let status = job_statuses.get(job_key);
            if status.map(|s| s.is_final()).unwrap_or(false) {
                if let Err(e) = airlock_core::remove_worktree(&repo.gate_path, worktree_path) {
                    warn!(
                        "Failed to remove worktree for job '{}' during DAG resume: {}",
                        job_key, e
                    );
                }
            }
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

/// Mark a run as cancelled (superseded by a newer push).
async fn mark_run_cancelled(ctx: &Arc<HandlerContext>, run: &Run) {
    let db = ctx.db.lock().await;
    let _ = db.update_run_error(&run.id, Some("Superseded by newer push"));
    drop(db);

    ctx.emit(AirlockEvent::RunCompleted {
        repo_id: run.repo_id.clone(),
        run_id: run.id.clone(),
        success: false,
    });
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
    use airlock_core::RefUpdate;

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
}
