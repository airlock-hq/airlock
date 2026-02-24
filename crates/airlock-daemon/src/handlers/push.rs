//! Push handlers.
//!
//! Handles push notifications and push coalescing.

use super::pipeline::{execute_pipeline, extract_branch_name, load_workflows_for_run};
use super::util::now;
use super::HandlerContext;
use crate::ipc::AirlockEvent;
use crate::push_coalescer;
use airlock_core::{git, RefUpdate, Repo, Run};
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Handle the `push_received` notification (from post-receive hook).
///
/// This function implements push coalescing and deduplication:
/// 1. Records the push in the coalescer for debouncing
/// 2. Checks for ready pushes (debounce period passed)
/// 3. For each ready push, supersedes overlapping runs and creates a new run
pub async fn handle_push_received(ctx: Arc<HandlerContext>, params: serde_json::Value) {
    use crate::ipc::PushReceivedParams;

    // Parse parameters
    let params: PushReceivedParams = match serde_json::from_value(params) {
        Ok(p) => p,
        Err(e) => {
            error!("Invalid push_received params: {}", e);
            return;
        }
    };

    debug!("Received push notification for gate: {}", params.gate_path);

    // Look up repo by gate path
    let gate_path = Path::new(&params.gate_path);
    let repo = {
        let db = ctx.db.lock().await;
        let repos = match db.list_repos() {
            Ok(r) => r,
            Err(e) => {
                error!("Failed to list repos: {}", e);
                return;
            }
        };

        repos.into_iter().find(|r| r.gate_path == gate_path)
    };

    let repo = match repo {
        Some(r) => r,
        None => {
            error!("No repo found for gate path: {}", params.gate_path);
            return;
        }
    };

    // Convert ref updates
    let ref_updates: Vec<RefUpdate> = params
        .ref_updates
        .into_iter()
        .map(|r| RefUpdate {
            ref_name: r.ref_name,
            old_sha: r.old_sha,
            new_sha: r.new_sha,
        })
        .collect();

    // Record push in coalescer (may return immediately if debounce period passed)
    let ready_refs = ctx
        .coalescer
        .record_push(&repo.id, ref_updates.clone())
        .await;

    // Also check for any other ready pushes (from other repos)
    let mut all_ready = ctx.coalescer.get_ready_pushes().await;
    if let Some(refs) = ready_refs {
        // This repo's push is ready immediately (debounce passed)
        all_ready.push((repo.id.clone(), refs));
    }

    // Process all ready pushes
    for (ready_repo_id, ready_refs) in all_ready {
        process_coalesced_push(ctx.clone(), &ready_repo_id, ready_refs).await;
    }
}

/// Process a coalesced push after the debounce period.
///
/// This partitions refs into:
/// - Pipeline refs (branch creates/updates) → go through transformation pipeline
/// - Passthrough refs (tags, deletions, other) → forwarded immediately to upstream
pub async fn process_coalesced_push(
    ctx: Arc<HandlerContext>,
    repo_id: &str,
    ref_updates: Vec<RefUpdate>,
) {
    debug!(
        "Processing coalesced push for repo {} with {} refs",
        repo_id,
        ref_updates.len()
    );

    // Get repo first (needed for passthrough forwarding)
    let repo = {
        let db = ctx.db.lock().await;
        match db.get_repo(repo_id) {
            Ok(Some(r)) => r,
            Ok(None) => {
                error!("Repo {} not found", repo_id);
                return;
            }
            Err(e) => {
                error!("Failed to get repo: {}", e);
                return;
            }
        }
    };

    // Partition refs into pipeline and passthrough
    let (pipeline_refs, passthrough_refs): (Vec<_>, Vec<_>) =
        ref_updates.iter().partition(|r| git::is_pipeline_ref(r));

    // Forward passthrough refs immediately (tags, deletions, other)
    if !passthrough_refs.is_empty() {
        forward_passthrough_refs(&repo, &passthrough_refs).await;
    }

    // If no pipeline refs, we're done
    if pipeline_refs.is_empty() {
        info!("No pipeline refs for repo {} - passthrough only", repo_id);
        return;
    }

    // Convert to owned refs for the run
    let mut pipeline_updates: Vec<RefUpdate> = pipeline_refs.into_iter().cloned().collect();

    // Supersede any overlapping active runs
    {
        let db = ctx.db.lock().await;
        match push_coalescer::supersede_overlapping_runs(&db, repo_id, &pipeline_updates) {
            Ok(superseded) => {
                if !superseded.is_empty() {
                    info!(
                        "Superseded {} overlapping run(s) for repo {}",
                        superseded.len(),
                        repo_id
                    );
                    // Inherit base_sha from superseded runs to avoid the
                    // "superseding gap" where changes get forwarded without review.
                    // For each superseded run, if it has a matching ref_name,
                    // replace our old_sha with the superseded run's base_sha.
                    for superseded_run in &superseded {
                        for update in pipeline_updates.iter_mut() {
                            let ref_matches = superseded_run
                                .ref_updates
                                .iter()
                                .any(|r| r.ref_name == update.ref_name);
                            if ref_matches {
                                info!(
                                    "Inheriting base_sha from superseded run {} for ref {}: {} -> {}",
                                    superseded_run.id,
                                    update.ref_name,
                                    &update.old_sha[..8.min(update.old_sha.len())],
                                    &superseded_run.base_sha[..8.min(superseded_run.base_sha.len())],
                                );
                                update.old_sha = superseded_run.base_sha.clone();
                            }
                        }
                    }
                }
            }
            Err(e) => {
                warn!("Failed to supersede overlapping runs: {}", e);
            }
        }
    }

    // Use upstream ref as base_sha to ensure we capture all un-forwarded commits.
    // Git's old_sha reflects the gate's previous state, but if a prior run failed,
    // upstream may be further behind. refs/remotes/origin/<branch> tracks the last
    // known upstream state.
    for update in pipeline_updates.iter_mut() {
        // Skip branch creations (null SHA) — no upstream to compare against
        if git::is_null_sha(&update.old_sha) {
            continue;
        }
        if let Some(branch) = update.ref_name.strip_prefix("refs/heads/") {
            let upstream_ref = format!("refs/remotes/origin/{}", branch);
            match git::resolve_ref(&repo.gate_path, &upstream_ref) {
                Ok(Some(upstream_sha)) if upstream_sha != update.old_sha => {
                    info!(
                        "Using upstream base for ref {}: {} (was {})",
                        update.ref_name,
                        &upstream_sha[..8.min(upstream_sha.len())],
                        &update.old_sha[..8.min(update.old_sha.len())],
                    );
                    update.old_sha = upstream_sha;
                }
                _ => {} // No upstream ref or same SHA — keep git's old_sha
            }
        }
    }

    // Extract branch, base_sha, and head_sha from pipeline_updates
    // Use the first ref update (all pipeline refs are branch creates/updates)
    let primary = &pipeline_updates[0];

    let branch = primary
        .ref_name
        .strip_prefix("refs/heads/")
        .unwrap_or(&primary.ref_name)
        .to_string();
    let base_sha = primary.old_sha.clone();
    let head_sha = primary.new_sha.clone();

    // Load matching workflows for this branch
    let branch_name = extract_branch_name(&pipeline_updates);
    let matching_workflows =
        match load_workflows_for_run(&repo.gate_path, &head_sha, branch_name.as_deref()) {
            Ok(w) => w,
            Err(e) => {
                // Fall back to creating a single run without workflow info
                warn!(
                    "Failed to load workflows: {}. Creating run without workflow info.",
                    e
                );
                vec![]
            }
        };

    // Create a run for each matching workflow (or one run if no workflows found)
    let workflow_runs: Vec<(Run, Option<String>)> = if matching_workflows.is_empty() {
        // No workflows or load failed — create a single run
        let created_at = now();
        let run = Run {
            id: uuid::Uuid::new_v4().to_string(),
            repo_id: repo_id.to_string(),
            ref_updates: pipeline_updates.clone(),
            error: None,
            superseded: false,
            created_at,
            branch: branch.clone(),
            base_sha: base_sha.clone(),
            head_sha: head_sha.clone(),
            current_step: None,
            updated_at: created_at,
            workflow_file: String::new(),
            workflow_name: None,
        };
        vec![(run, None)]
    } else {
        matching_workflows
            .iter()
            .map(|(filename, wf)| {
                let created_at = now();
                let run = Run {
                    id: uuid::Uuid::new_v4().to_string(),
                    repo_id: repo_id.to_string(),
                    ref_updates: pipeline_updates.clone(),
                    error: None,
                    superseded: false,
                    created_at,
                    branch: branch.clone(),
                    base_sha: base_sha.clone(),
                    head_sha: head_sha.clone(),
                    current_step: None,
                    updated_at: created_at,
                    workflow_file: filename.clone(),
                    workflow_name: wf.name.clone(),
                };
                (run, Some(filename.clone()))
            })
            .collect()
    };

    for (run, _workflow_file) in workflow_runs {
        {
            let db = ctx.db.lock().await;
            if let Err(e) = db.insert_run(&run) {
                error!("Failed to insert run: {}", e);
                continue;
            }
        }

        // Create protective ref to prevent GC of run commits
        let protective_ref = git::run_ref(&run.id);
        if let Err(e) = git::update_ref(&repo.gate_path, &protective_ref, &run.head_sha) {
            warn!("Failed to create protective ref for run {}: {}", run.id, e);
        } else {
            debug!(
                "Created protective ref {} -> {}",
                protective_ref,
                &run.head_sha[..8.min(run.head_sha.len())]
            );
        }

        info!(
            "Created run {} for repo {} (workflow: {}) with {} ref updates",
            run.id,
            run.repo_id,
            if run.workflow_file.is_empty() {
                "default"
            } else {
                &run.workflow_file
            },
            run.ref_updates.len()
        );

        // Emit RunCreated event
        ctx.emit(AirlockEvent::RunCreated {
            repo_id: run.repo_id.clone(),
            run_id: run.id.clone(),
            branch: run.branch.clone(),
        });

        // Spawn the pipeline through the run queue so that:
        // 1. Only one run at a time per repo (serialized via semaphore)
        // 2. A newer push cancels the active run for the same repo
        let ctx = ctx.clone();
        let repo = repo.clone();
        tokio::spawn(async move {
            let permit = ctx.run_queue.acquire(&run.repo_id).await;
            execute_pipeline(ctx.clone(), run, repo, permit.token).await;
            // permit is dropped here, releasing the slot for the next run
        });
    }
}

/// Forward passthrough refs directly to upstream.
///
/// Passthrough refs include tags, branch deletions, and other refs (notes, etc.).
/// These are forwarded immediately without going through the transformation pipeline.
async fn forward_passthrough_refs(repo: &Repo, refs: &[&RefUpdate]) {
    // Log by type for clarity
    let tags: Vec<_> = refs
        .iter()
        .filter(|r| r.ref_name.starts_with("refs/tags/"))
        .collect();
    let deletions: Vec<_> = refs
        .iter()
        .filter(|r| git::is_null_sha(&r.new_sha) && r.ref_name.starts_with("refs/heads/"))
        .collect();
    let other: Vec<_> = refs
        .iter()
        .filter(|r| !r.ref_name.starts_with("refs/heads/") && !r.ref_name.starts_with("refs/tags/"))
        .collect();

    if !tags.is_empty() {
        info!("Forwarding {} tag(s) to upstream", tags.len());
    }
    if !deletions.is_empty() {
        info!(
            "Forwarding {} branch deletion(s) to upstream",
            deletions.len()
        );
    }
    if !other.is_empty() {
        info!("Forwarding {} other ref(s) to upstream", other.len());
    }

    // Push to origin (errors logged but don't block)
    if let Err(e) = git::push_ref_updates(&repo.gate_path, "origin", refs) {
        error!(
            "Failed to forward passthrough refs: {}. Refs are in gate but not upstream.",
            e
        );
    }
}

/// Detect and process any pushes that were missed while the daemon was down.
///
/// This function is called during daemon startup to handle the case where:
/// 1. User pushed to the gate while the daemon was not running
/// 2. The post-receive hook notification was lost (daemon wasn't listening)
/// 3. The commits are in the gate but no pipeline was triggered
///
/// For each enrolled repo, it compares the gate's branch HEADs with upstream's
/// and triggers pipelines for any branches that are ahead of upstream.
pub async fn detect_and_process_missed_pushes(ctx: Arc<HandlerContext>) {
    info!("Checking for missed pushes while daemon was down...");

    // Get all enrolled repos
    let repos = {
        let db = ctx.db.lock().await;
        match db.list_repos() {
            Ok(r) => r,
            Err(e) => {
                warn!("Failed to list repos for missed push detection: {}", e);
                return;
            }
        }
    };

    if repos.is_empty() {
        debug!("No enrolled repos, skipping missed push detection");
        return;
    }

    let mut total_missed = 0;

    for repo in repos {
        match detect_missed_pushes_for_repo(&ctx, &repo).await {
            Ok(count) => {
                total_missed += count;
            }
            Err(e) => {
                warn!("Failed to detect missed pushes for repo {}: {}", repo.id, e);
            }
        }
    }

    if total_missed > 0 {
        info!(
            "Detected and queued {} missed push(es) for processing",
            total_missed
        );
    } else {
        debug!("No missed pushes detected");
    }
}

/// Detect missed pushes for a single repo.
///
/// Returns the number of missed pushes detected and queued.
async fn detect_missed_pushes_for_repo(
    ctx: &Arc<HandlerContext>,
    repo: &Repo,
) -> Result<usize, String> {
    // Open the gate repo
    let gate_repo =
        git::open_repo(&repo.gate_path).map_err(|e| format!("Failed to open gate repo: {}", e))?;

    // Get all branch refs from the gate
    let mut missed_refs: Vec<RefUpdate> = Vec::new();

    // Iterate over all references in the gate
    let refs = gate_repo
        .references()
        .map_err(|e| format!("Failed to list gate refs: {}", e))?;

    for reference in refs.flatten() {
        let ref_name = match reference.name() {
            Some(name) => name.to_string(),
            None => continue,
        };

        // Only process branch refs (refs/heads/*)
        if !ref_name.starts_with("refs/heads/") {
            continue;
        }

        // Get the current commit SHA
        let gate_sha = match reference.peel_to_commit() {
            Ok(commit) => commit.id().to_string(),
            Err(_) => continue,
        };

        // Check if there's already an active run covering this ref
        let has_active_run = {
            let db = ctx.db.lock().await;
            match db.list_active_runs(&repo.id) {
                Ok(runs) => runs.iter().any(|run| {
                    run.ref_updates
                        .iter()
                        .any(|u| u.ref_name == ref_name && u.new_sha == gate_sha)
                }),
                Err(_) => false,
            }
        };

        if has_active_run {
            // Already being processed
            continue;
        }

        // Check if there's a completed (or failed) run that already processed this exact state
        let already_processed = {
            let db = ctx.db.lock().await;
            match db.list_runs(&repo.id, Some(50)) {
                Ok(runs) => runs.iter().any(|run| {
                    // Check if this run processed this ref to this SHA
                    let ref_matches = run
                        .ref_updates
                        .iter()
                        .any(|u| u.ref_name == ref_name && u.new_sha == gate_sha);
                    if !ref_matches {
                        return false;
                    }
                    // Check step-based completion
                    let step_completed = match db.get_step_results_for_run(&run.id) {
                        Ok(stages) => run.is_completed(&stages),
                        Err(_) => false,
                    };
                    if step_completed {
                        return true;
                    }
                    // Also check job-based completion as defense in depth:
                    // a run where all jobs reached final status (Failed/Passed/Skipped)
                    // should not be re-triggered even if some steps were left Pending
                    match db.get_job_results_for_run(&run.id) {
                        Ok(jobs) => run.is_completed_from_jobs(&jobs),
                        Err(_) => false,
                    }
                }),
                Err(_) => false,
            }
        };

        if already_processed {
            // Already processed in a previous run
            continue;
        }

        // Get the origin SHA for comparison
        // The origin remote in the gate should have the same branch
        let upstream_ref_name = format!("refs/remotes/origin/{}", &ref_name[11..]); // Strip "refs/heads/"
        let upstream_sha = gate_repo
            .find_reference(&upstream_ref_name)
            .ok()
            .and_then(|r| r.peel_to_commit().ok())
            .map(|c| c.id().to_string());

        // If gate SHA differs from upstream SHA (or upstream doesn't have it),
        // we have a missed push
        let is_missed = match &upstream_sha {
            Some(u_sha) => u_sha != &gate_sha,
            None => true, // New branch that upstream doesn't have
        };

        if is_missed {
            let old_sha = upstream_sha.unwrap_or_else(|| {
                // For new branches, use the zero SHA or try to find merge-base
                "0000000000000000000000000000000000000000".to_string()
            });

            info!(
                "Detected missed push for repo {}: {} ({} -> {})",
                repo.id,
                ref_name,
                &old_sha[..8.min(old_sha.len())],
                &gate_sha[..8]
            );

            missed_refs.push(RefUpdate {
                ref_name,
                old_sha,
                new_sha: gate_sha,
            });
        }
    }

    if missed_refs.is_empty() {
        return Ok(0);
    }

    let count = missed_refs.len();

    // Process the missed push (similar to process_coalesced_push)
    process_coalesced_push(ctx.clone(), &repo.id, missed_refs).await;

    Ok(count)
}

/// Process any ready pushes from the coalescer.
///
/// This should be called periodically to ensure pushes are processed
/// even if no new pushes arrive.
pub async fn process_ready_pushes(ctx: Arc<HandlerContext>) {
    let ready = ctx.coalescer.get_ready_pushes().await;
    for (repo_id, ref_updates) in ready {
        process_coalesced_push(ctx.clone(), &repo_id, ref_updates).await;
    }
}

/// Clean up protective refs for runs that have been forwarded or are no longer active.
///
/// This should be called at daemon startup to remove stale refs/airlock/runs/* refs
/// for runs that have completed (forwarded to upstream) or been superseded.
pub async fn cleanup_stale_run_refs(ctx: Arc<HandlerContext>) {
    info!("Cleaning up stale run refs...");

    let repos = {
        let db = ctx.db.lock().await;
        match db.list_repos() {
            Ok(r) => r,
            Err(e) => {
                warn!("Failed to list repos for run ref cleanup: {}", e);
                return;
            }
        }
    };

    let mut cleaned = 0;

    for repo in repos {
        // List all refs/airlock/runs/* refs in the gate
        let output = std::process::Command::new("git")
            .args(["-C", repo.gate_path.to_str().unwrap_or(".")])
            .args(["for-each-ref", "--format=%(refname)", "refs/airlock/runs/"])
            .output();

        let output = match output {
            Ok(o) if o.status.success() => o,
            _ => continue,
        };

        let refs_output = String::from_utf8_lossy(&output.stdout);
        for ref_line in refs_output.lines() {
            let ref_name = ref_line.trim();
            if ref_name.is_empty() {
                continue;
            }

            // Extract run_id from refs/airlock/runs/{run_id}
            let run_id = match ref_name.strip_prefix("refs/airlock/runs/") {
                Some(id) => id,
                None => continue,
            };

            // Check if the run is still active
            let should_clean = {
                let db = ctx.db.lock().await;
                match db.get_run(run_id) {
                    Ok(Some(run)) => {
                        // Clean up if superseded or if all steps/jobs are complete
                        if run.superseded {
                            true
                        } else {
                            match db.get_job_results_for_run(run_id) {
                                Ok(jobs) => run.is_completed_from_jobs(&jobs),
                                Err(_) => false,
                            }
                        }
                    }
                    Ok(None) => true, // Run not found — stale ref
                    Err(_) => false,
                }
            };

            if should_clean {
                if let Err(e) = git::delete_ref(&repo.gate_path, ref_name) {
                    warn!("Failed to delete stale run ref {}: {}", ref_name, e);
                } else {
                    debug!("Cleaned up stale run ref: {}", ref_name);
                    cleaned += 1;
                }
            }
        }
    }

    if cleaned > 0 {
        info!("Cleaned up {} stale run ref(s)", cleaned);
    } else {
        debug!("No stale run refs to clean up");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use airlock_core::{AirlockPaths, Database, JobResult, JobStatus, StepResult, StepStatus};
    use std::path::PathBuf;
    use tempfile::TempDir;
    use tokio::sync::watch;

    fn create_test_context() -> Arc<HandlerContext> {
        let paths = AirlockPaths::with_root(PathBuf::from("/tmp/airlock-test-push"));
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

    #[tokio::test]
    async fn test_process_coalesced_push_passthrough_deletion_only() {
        let ctx = create_test_context();

        // Set up a test repo
        {
            let db = ctx.db.lock().await;
            let repo = create_test_repo("repo1");
            db.insert_repo(&repo).unwrap();
        }

        // Process a push with only branch deletions (new_sha is all zeros)
        // This should be classified as passthrough and no run should be created
        let ref_updates = vec![RefUpdate {
            ref_name: "refs/heads/deleted-branch".to_string(),
            old_sha: "abc123def456".to_string(),
            new_sha: "0000000000000000000000000000000000000000".to_string(),
        }];

        process_coalesced_push(ctx.clone(), "repo1", ref_updates).await;

        // Verify no run was created (deletions are passthrough, not pipeline)
        let runs = {
            let db = ctx.db.lock().await;
            db.list_runs("repo1", None).unwrap()
        };
        assert!(
            runs.is_empty(),
            "No run should be created for deletion-only push (passthrough)"
        );
    }

    #[tokio::test]
    async fn test_process_coalesced_push_creates_run_for_valid_push() {
        let ctx = create_test_context();

        // Set up a test repo
        {
            let db = ctx.db.lock().await;
            let repo = create_test_repo("repo2");
            db.insert_repo(&repo).unwrap();
        }

        // Process a push with a valid branch update
        let ref_updates = vec![RefUpdate {
            ref_name: "refs/heads/feature-branch".to_string(),
            old_sha: "abc123def456".to_string(),
            new_sha: "def456abc789".to_string(),
        }];

        process_coalesced_push(ctx.clone(), "repo2", ref_updates).await;

        // Verify a run was created
        let runs = {
            let db = ctx.db.lock().await;
            db.list_runs("repo2", None).unwrap()
        };
        assert_eq!(runs.len(), 1, "A run should be created for valid push");
        assert_eq!(runs[0].branch, "feature-branch");
    }

    #[tokio::test]
    async fn test_process_coalesced_push_with_mixed_refs() {
        let ctx = create_test_context();

        // Set up a test repo
        {
            let db = ctx.db.lock().await;
            let repo = create_test_repo("repo3");
            db.insert_repo(&repo).unwrap();
        }

        // Process a push with both a deletion (passthrough) and a valid update (pipeline)
        let ref_updates = vec![
            RefUpdate {
                ref_name: "refs/heads/deleted-branch".to_string(),
                old_sha: "abc123".to_string(),
                new_sha: "0000000000000000000000000000000000000000".to_string(),
            },
            RefUpdate {
                ref_name: "refs/heads/new-branch".to_string(),
                old_sha: "0000000000000000000000000000000000000000".to_string(),
                new_sha: "def456abc789".to_string(),
            },
        ];

        process_coalesced_push(ctx.clone(), "repo3", ref_updates).await;

        // Verify a run was created using only the pipeline ref (branch update)
        // The deletion is passthrough and not included in the run
        let runs = {
            let db = ctx.db.lock().await;
            db.list_runs("repo3", None).unwrap()
        };
        assert_eq!(runs.len(), 1, "A run should be created for mixed push");
        assert_eq!(runs[0].branch, "new-branch");
        assert_eq!(runs[0].head_sha, "def456abc789");
        // Verify the run only contains the pipeline ref, not the deletion
        assert_eq!(runs[0].ref_updates.len(), 1);
        assert_eq!(runs[0].ref_updates[0].ref_name, "refs/heads/new-branch");
    }

    #[tokio::test]
    async fn test_process_coalesced_push_passthrough_tags_only() {
        let ctx = create_test_context();

        // Set up a test repo
        {
            let db = ctx.db.lock().await;
            let repo = create_test_repo("repo4");
            db.insert_repo(&repo).unwrap();
        }

        // Process a push with only tags (passthrough, no pipeline)
        let ref_updates = vec![
            RefUpdate {
                ref_name: "refs/tags/v1.0.0".to_string(),
                old_sha: "0000000000000000000000000000000000000000".to_string(),
                new_sha: "abc123def456".to_string(),
            },
            RefUpdate {
                ref_name: "refs/tags/v1.0.1".to_string(),
                old_sha: "0000000000000000000000000000000000000000".to_string(),
                new_sha: "def456abc789".to_string(),
            },
        ];

        process_coalesced_push(ctx.clone(), "repo4", ref_updates).await;

        // Verify no run was created (tags are passthrough, not pipeline)
        let runs = {
            let db = ctx.db.lock().await;
            db.list_runs("repo4", None).unwrap()
        };
        assert!(
            runs.is_empty(),
            "No run should be created for tags-only push (passthrough)"
        );
    }

    #[tokio::test]
    async fn test_process_coalesced_push_mixed_with_tags_and_branches() {
        let ctx = create_test_context();

        // Set up a test repo
        {
            let db = ctx.db.lock().await;
            let repo = create_test_repo("repo5");
            db.insert_repo(&repo).unwrap();
        }

        // Process a push with tags (passthrough) and branch update (pipeline)
        let ref_updates = vec![
            RefUpdate {
                ref_name: "refs/tags/v1.0.0".to_string(),
                old_sha: "0000000000000000000000000000000000000000".to_string(),
                new_sha: "abc123def456".to_string(),
            },
            RefUpdate {
                ref_name: "refs/heads/release".to_string(),
                old_sha: "abc123def456".to_string(),
                new_sha: "def456abc789".to_string(),
            },
        ];

        process_coalesced_push(ctx.clone(), "repo5", ref_updates).await;

        // Verify a run was created only for the branch update
        let runs = {
            let db = ctx.db.lock().await;
            db.list_runs("repo5", None).unwrap()
        };
        assert_eq!(runs.len(), 1, "A run should be created for mixed push");
        assert_eq!(runs[0].branch, "release");
        // Verify the run only contains the pipeline ref (branch), not the tag
        assert_eq!(runs[0].ref_updates.len(), 1);
        assert_eq!(runs[0].ref_updates[0].ref_name, "refs/heads/release");
    }

    /// Regression test: a failed run where some steps are still Pending (due to
    /// early break in execute_single_job) should NOT be picked up by
    /// list_active_runs, and SHOULD be caught by the job-based already_processed
    /// check so it doesn't get re-triggered.
    #[tokio::test]
    async fn test_failed_run_with_pending_steps_not_retriggered() {
        let ctx = create_test_context();

        let repo = create_test_repo("repo1");
        {
            let db = ctx.db.lock().await;
            db.insert_repo(&repo).unwrap();
        }

        // Create a run that was processed (same ref/sha as we'll check)
        let run = Run {
            id: "run1".to_string(),
            repo_id: "repo1".to_string(),
            ref_updates: vec![RefUpdate {
                ref_name: "refs/heads/main".to_string(),
                old_sha: "aaa".to_string(),
                new_sha: "bbb".to_string(),
            }],
            branch: "main".to_string(),
            base_sha: "aaa".to_string(),
            head_sha: "bbb".to_string(),
            current_step: None,
            error: Some("Pipeline interrupted".to_string()),
            superseded: false,
            workflow_file: "main.yml".to_string(),
            workflow_name: None,
            created_at: 1704067200,
            updated_at: 1704067200,
        };

        {
            let db = ctx.db.lock().await;
            db.insert_run(&run).unwrap();

            // Job completed as Failed (final status)
            let job = JobResult {
                id: "job1".to_string(),
                run_id: "run1".to_string(),
                job_key: "build".to_string(),
                name: Some("Build".to_string()),
                status: JobStatus::Failed,
                job_order: 0,
                started_at: Some(1704067200),
                completed_at: Some(1704067210),
                error: Some("step failed".to_string()),
            };
            db.insert_job_result(&job).unwrap();

            // Step 1: Failed
            let step1 = StepResult {
                id: "s1".to_string(),
                run_id: "run1".to_string(),
                job_id: "job1".to_string(),
                name: "lint".to_string(),
                status: StepStatus::Failed,
                step_order: 0,
                exit_code: Some(1),
                duration_ms: None,
                error: Some("lint failed".to_string()),
                started_at: Some(1704067200),
                completed_at: Some(1704067205),
            };
            db.insert_step_result(&step1).unwrap();

            // Step 2: still Pending (the old bug - not marked as Skipped)
            let step2 = StepResult {
                id: "s2".to_string(),
                run_id: "run1".to_string(),
                job_id: "job1".to_string(),
                name: "test".to_string(),
                status: StepStatus::Pending,
                step_order: 1,
                exit_code: None,
                duration_ms: None,
                error: None,
                started_at: None,
                completed_at: None,
            };
            db.insert_step_result(&step2).unwrap();
        }

        // Verify list_active_runs does NOT include this run
        // (job is Failed which is final, so is_running_from_jobs should be false)
        {
            let db = ctx.db.lock().await;
            let active = db.list_active_runs("repo1").unwrap();
            assert!(
                active.is_empty(),
                "A run with all jobs in final status should not be active"
            );
        }

        // Verify the job-based completion check catches it
        {
            let db = ctx.db.lock().await;
            let jobs = db.get_job_results_for_run("run1").unwrap();
            assert!(
                run.is_completed_from_jobs(&jobs),
                "Job-based check should see run as completed when all jobs are final"
            );

            // But step-based check would miss it (the old bug)
            let steps = db.get_step_results_for_run("run1").unwrap();
            assert!(
                !run.is_completed(&steps),
                "Step-based check incorrectly sees run as incomplete due to Pending step"
            );
        }
    }

    /// Create a commit in a bare repo using git2, returning the commit SHA.
    fn create_bare_repo_commit(repo: &git2::Repository) -> String {
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let oid = repo
            .commit(Some("refs/heads/main"), &sig, &sig, "initial", &tree, &[])
            .unwrap();
        oid.to_string()
    }

    /// Integration test: exercises the real `detect_and_process_missed_pushes`
    /// code path with a real git gate repo and a failed run that has leftover
    /// Pending steps.
    ///
    /// Before the fix, this would create a duplicate run because the step-based
    /// `is_completed` check returned false (Pending steps). After the fix, the
    /// job-based fallback catches it.
    #[tokio::test]
    async fn test_detect_missed_pushes_skips_failed_run_with_pending_steps() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("airlock");
        std::fs::create_dir_all(&root).unwrap();

        let paths = AirlockPaths::with_root(root);

        // Create a real bare repo to serve as the gate
        let gate_path = temp_dir.path().join("gate.git");
        let gate_repo = git2::Repository::init_bare(&gate_path).unwrap();

        // Add an "origin" remote (required by detect_missed_pushes_for_repo)
        // Points to a dummy path — we never actually fetch from it
        gate_repo.remote("origin", "file:///dev/null").unwrap();

        // Create a commit on refs/heads/main in the gate
        let head_sha = create_bare_repo_commit(&gate_repo);

        // Set up the database
        let db = Database::open_in_memory().unwrap();

        let repo_id = "test-repo";
        let repo = Repo {
            id: repo_id.to_string(),
            working_path: temp_dir.path().to_path_buf(),
            upstream_url: "file:///dev/null".to_string(),
            gate_path: gate_path.clone(),
            last_sync: None,
            created_at: 1704067200,
        };
        db.insert_repo(&repo).unwrap();

        // Create a failed run with the SAME ref+SHA as the gate's current state.
        // This simulates a run that processed this exact push and failed.
        let run = Run {
            id: "run-failed".to_string(),
            repo_id: repo_id.to_string(),
            ref_updates: vec![RefUpdate {
                ref_name: "refs/heads/main".to_string(),
                old_sha: "0000000000000000000000000000000000000000".to_string(),
                new_sha: head_sha.clone(),
            }],
            branch: "main".to_string(),
            base_sha: "0000000000000000000000000000000000000000".to_string(),
            head_sha: head_sha.clone(),
            current_step: None,
            error: Some("Pipeline interrupted".to_string()),
            superseded: false,
            workflow_file: "main.yml".to_string(),
            workflow_name: None,
            created_at: 1704067200,
            updated_at: 1704067200,
        };
        db.insert_run(&run).unwrap();

        // Job is Failed (final status)
        let job = JobResult {
            id: "job1".to_string(),
            run_id: "run-failed".to_string(),
            job_key: "build".to_string(),
            name: Some("Build".to_string()),
            status: JobStatus::Failed,
            job_order: 0,
            started_at: Some(1704067200),
            completed_at: Some(1704067210),
            error: Some("step failed".to_string()),
        };
        db.insert_job_result(&job).unwrap();

        // Step 1: Failed
        db.insert_step_result(&StepResult {
            id: "s1".to_string(),
            run_id: "run-failed".to_string(),
            job_id: "job1".to_string(),
            name: "lint".to_string(),
            status: StepStatus::Failed,
            step_order: 0,
            exit_code: Some(1),
            duration_ms: None,
            error: Some("lint failed".to_string()),
            started_at: Some(1704067200),
            completed_at: Some(1704067205),
        })
        .unwrap();

        // Step 2: Pending — this is the crux of the bug.
        // Before the fix, this caused is_completed() to return false,
        // making detect_missed_pushes think the push was never processed.
        db.insert_step_result(&StepResult {
            id: "s2".to_string(),
            run_id: "run-failed".to_string(),
            job_id: "job1".to_string(),
            name: "test".to_string(),
            status: StepStatus::Pending,
            step_order: 1,
            exit_code: None,
            duration_ms: None,
            error: None,
            started_at: None,
            completed_at: None,
        })
        .unwrap();

        // Now run the actual detect_and_process_missed_pushes
        let (shutdown_tx, _) = watch::channel(false);
        let ctx = Arc::new(HandlerContext::new(paths, db, shutdown_tx));

        detect_and_process_missed_pushes(ctx.clone()).await;

        // Verify: no new run should have been created.
        // Before the fix, a duplicate "run-failed" would appear as a second run.
        let runs = {
            let db = ctx.db.lock().await;
            db.list_runs(repo_id, None).unwrap()
        };
        assert_eq!(
            runs.len(),
            1,
            "Expected exactly 1 run (the original failed one), but found {}. \
             A duplicate run was incorrectly created for an already-failed push.",
            runs.len()
        );
        assert_eq!(runs[0].id, "run-failed");
    }

    /// E2E: process_coalesced_push creates a protective ref for the run.
    ///
    /// When a pipeline run is created, a ref at refs/airlock/runs/{run_id}
    /// should be created pointing to the head_sha to prevent GC.
    #[tokio::test]
    async fn test_process_coalesced_push_creates_protective_ref() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("airlock");
        std::fs::create_dir_all(&root).unwrap();

        let paths = AirlockPaths::with_root(root);

        // Create a real bare gate repo
        let gate_path = temp_dir.path().join("gate.git");
        let gate_repo = git2::Repository::init_bare(&gate_path).unwrap();
        gate_repo.remote("origin", "file:///dev/null").unwrap();

        // Create a commit on refs/heads/main
        let head_sha = create_bare_repo_commit(&gate_repo);

        // Set up database with repo
        let db = Database::open_in_memory().unwrap();
        let repo = Repo {
            id: "repo-prot".to_string(),
            working_path: temp_dir.path().to_path_buf(),
            upstream_url: "file:///dev/null".to_string(),
            gate_path: gate_path.clone(),
            last_sync: None,
            created_at: 1704067200,
        };
        db.insert_repo(&repo).unwrap();

        let (shutdown_tx, _) = watch::channel(false);
        let ctx = Arc::new(HandlerContext::new(paths, db, shutdown_tx));

        // Process a push
        let ref_updates = vec![RefUpdate {
            ref_name: "refs/heads/main".to_string(),
            old_sha: "0000000000000000000000000000000000000000".to_string(),
            new_sha: head_sha.clone(),
        }];

        process_coalesced_push(ctx.clone(), "repo-prot", ref_updates).await;

        // A run should have been created
        let runs = {
            let db = ctx.db.lock().await;
            db.list_runs("repo-prot", None).unwrap()
        };
        assert_eq!(runs.len(), 1, "A run should be created");

        // Verify the protective ref exists
        let run_id = &runs[0].id;
        let protective_ref = git::run_ref(run_id);
        let resolved = git::resolve_ref(&gate_path, &protective_ref).unwrap();
        assert_eq!(
            resolved,
            Some(head_sha.clone()),
            "Protective ref should point to the run's head_sha"
        );
    }

    /// E2E: cleanup_stale_run_refs removes refs for completed runs.
    ///
    /// After a run's job is marked as completed (Passed), the protective ref
    /// should be cleaned up at startup.
    #[tokio::test]
    async fn test_cleanup_stale_run_refs_removes_completed() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("airlock");
        std::fs::create_dir_all(&root).unwrap();

        let paths = AirlockPaths::with_root(root);

        // Create a real bare gate repo
        let gate_path = temp_dir.path().join("gate.git");
        let gate_repo = git2::Repository::init_bare(&gate_path).unwrap();
        gate_repo.remote("origin", "file:///dev/null").unwrap();

        let head_sha = create_bare_repo_commit(&gate_repo);

        // Set up database
        let db = Database::open_in_memory().unwrap();
        let repo = Repo {
            id: "repo-clean".to_string(),
            working_path: temp_dir.path().to_path_buf(),
            upstream_url: "file:///dev/null".to_string(),
            gate_path: gate_path.clone(),
            last_sync: None,
            created_at: 1704067200,
        };
        db.insert_repo(&repo).unwrap();

        // Create a completed run
        let run = Run {
            id: "run-completed".to_string(),
            repo_id: "repo-clean".to_string(),
            ref_updates: vec![RefUpdate {
                ref_name: "refs/heads/main".to_string(),
                old_sha: "0000000000000000000000000000000000000000".to_string(),
                new_sha: head_sha.clone(),
            }],
            branch: "main".to_string(),
            base_sha: "0000000000000000000000000000000000000000".to_string(),
            head_sha: head_sha.clone(),
            current_step: None,
            error: None,
            superseded: false,
            workflow_file: "main.yml".to_string(),
            workflow_name: None,
            created_at: 1704067200,
            updated_at: 1704067200,
        };
        db.insert_run(&run).unwrap();

        // Mark the job as Passed (completed)
        let job = JobResult {
            id: "job-done".to_string(),
            run_id: "run-completed".to_string(),
            job_key: "default".to_string(),
            name: Some("Default".to_string()),
            status: JobStatus::Passed,
            job_order: 0,
            started_at: Some(1704067200),
            completed_at: Some(1704067210),
            error: None,
        };
        db.insert_job_result(&job).unwrap();

        // Create the protective ref manually (simulating what push handler does)
        let protective_ref = git::run_ref("run-completed");
        git::update_ref(&gate_path, &protective_ref, &head_sha).unwrap();

        // Also create a ref for a run that doesn't exist in DB (stale)
        let stale_ref = git::run_ref("run-nonexistent");
        git::update_ref(&gate_path, &stale_ref, &head_sha).unwrap();

        // Verify both refs exist before cleanup
        assert!(git::resolve_ref(&gate_path, &protective_ref)
            .unwrap()
            .is_some());
        assert!(git::resolve_ref(&gate_path, &stale_ref).unwrap().is_some());

        // Run cleanup
        let (shutdown_tx, _) = watch::channel(false);
        let ctx = Arc::new(HandlerContext::new(paths, db, shutdown_tx));
        cleanup_stale_run_refs(ctx).await;

        // Both refs should be cleaned up
        // - run-completed: job is Passed (final status)
        // - run-nonexistent: not found in DB
        assert!(
            git::resolve_ref(&gate_path, &protective_ref)
                .unwrap()
                .is_none(),
            "Completed run's protective ref should be cleaned up"
        );
        assert!(
            git::resolve_ref(&gate_path, &stale_ref).unwrap().is_none(),
            "Stale (no DB record) protective ref should be cleaned up"
        );
    }

    /// Create a child commit on refs/heads/main in a bare repo, returning the new SHA.
    fn create_child_commit(repo: &git2::Repository) -> String {
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let parent = repo
            .find_reference("refs/heads/main")
            .unwrap()
            .peel_to_commit()
            .unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let oid = repo
            .commit(
                Some("refs/heads/main"),
                &sig,
                &sig,
                "child commit",
                &tree,
                &[&parent],
            )
            .unwrap();
        oid.to_string()
    }

    /// E2E: After a failed run, the next push should use the upstream SHA as
    /// base_sha rather than the gate's old_sha.
    ///
    /// Scenario:
    /// 1. Push commit1 (base=upstream_sha, head=commit1) → Run1 fails
    /// 2. Push commit2 (git says old_sha=commit1, new_sha=commit2)
    /// 3. Run2 should have base_sha=upstream_sha, not commit1
    #[tokio::test]
    async fn test_failed_run_next_push_uses_upstream_base_sha() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("airlock");
        std::fs::create_dir_all(&root).unwrap();

        let paths = AirlockPaths::with_root(root);

        // Create a real bare gate repo
        let gate_path = temp_dir.path().join("gate.git");
        let gate_repo = git2::Repository::init_bare(&gate_path).unwrap();
        gate_repo.remote("origin", "file:///dev/null").unwrap();

        // Create the initial commit (this represents what's on upstream)
        let upstream_sha = create_bare_repo_commit(&gate_repo);

        // Set refs/remotes/origin/main to the upstream SHA
        // This simulates what `git fetch` would do — tracking the upstream state
        git::update_ref(&gate_path, "refs/remotes/origin/main", &upstream_sha).unwrap();

        // Create commit1 on top (simulates user's first push)
        let commit1_sha = create_child_commit(&gate_repo);

        // Set up database
        let db = Database::open_in_memory().unwrap();
        let repo = Repo {
            id: "repo-upstream".to_string(),
            working_path: temp_dir.path().to_path_buf(),
            upstream_url: "file:///dev/null".to_string(),
            gate_path: gate_path.clone(),
            last_sync: None,
            created_at: 1704067200,
        };
        db.insert_repo(&repo).unwrap();

        let (shutdown_tx, _) = watch::channel(false);
        let ctx = Arc::new(HandlerContext::new(paths, db, shutdown_tx));

        // --- Push 1: commit1, which creates Run1 ---
        let ref_updates1 = vec![RefUpdate {
            ref_name: "refs/heads/main".to_string(),
            old_sha: upstream_sha.clone(),
            new_sha: commit1_sha.clone(),
        }];
        process_coalesced_push(ctx.clone(), "repo-upstream", ref_updates1).await;

        // Verify Run1 was created with correct base_sha
        let runs1 = {
            let db = ctx.db.lock().await;
            db.list_runs("repo-upstream", None).unwrap()
        };
        assert_eq!(runs1.len(), 1);
        assert_eq!(
            runs1[0].base_sha, upstream_sha,
            "Run1 should use upstream as base"
        );
        assert_eq!(runs1[0].head_sha, commit1_sha);

        // Simulate Run1 failing — mark job as Failed so it's not "active"
        {
            let db = ctx.db.lock().await;
            let job = JobResult {
                id: "job-fail".to_string(),
                run_id: runs1[0].id.clone(),
                job_key: "build".to_string(),
                name: Some("Build".to_string()),
                status: JobStatus::Failed,
                job_order: 0,
                started_at: Some(1704067200),
                completed_at: Some(1704067210),
                error: Some("lint failed".to_string()),
            };
            db.insert_job_result(&job).unwrap();
        }

        // --- Push 2: commit2 on top of commit1 ---
        // Git's post-receive would report old_sha=commit1 (gate's previous HEAD)
        let commit2_sha = create_child_commit(&gate_repo);

        let ref_updates2 = vec![RefUpdate {
            ref_name: "refs/heads/main".to_string(),
            old_sha: commit1_sha.clone(), // Git says the old ref was commit1
            new_sha: commit2_sha.clone(),
        }];
        process_coalesced_push(ctx.clone(), "repo-upstream", ref_updates2).await;

        // Verify Run2 was created with upstream_sha as base, NOT commit1
        let runs2 = {
            let db = ctx.db.lock().await;
            db.list_runs("repo-upstream", None).unwrap()
        };
        assert_eq!(runs2.len(), 2, "Two runs should exist");

        // Find the second run (the one for commit2)
        let run2 = runs2.iter().find(|r| r.head_sha == commit2_sha).unwrap();
        assert_eq!(
            run2.base_sha, upstream_sha,
            "Run2 should use upstream SHA as base (not commit1), \
             so the diff covers all un-forwarded commits"
        );
    }
}
