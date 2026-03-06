//! Push stage implementation — two-phase push: worktree -> gate -> upstream.
//!
//! This replaces the shell-based push stage to fix the gate ref desync bug:
//! when `freeze` creates a commit in the worktree, the old push script pushed
//! directly from the worktree to upstream via URL, bypassing the gate. The gate
//! never learned about the new commit, causing subsequent pushes to fail with
//! "non-fast-forward."
//!
//! Push flow:
//! 1. Get worktree HEAD (may include freeze commit)
//! 2. Sync gate from upstream: `git fetch origin` (in gate)
//! 3. Get upstream HEAD from gate's remote tracking ref
//! 4. Check: is upstream HEAD an ancestor of worktree HEAD?
//!    YES → regular push (fast-forward)
//!    NO  → check rebase_state.json from rebase stage:
//!      - If rebase recorded the same upstream SHA → force-with-lease (safe)
//!      - If upstream moved since rebase, or no rebase state → hard failure
//!
//! Usage:
//!   airlock exec push

use airlock_core::git;
use airlock_core::AirlockPaths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use crate::commands::ipc_client;

/// State written by the rebase stage (`rebase_state.json`) so push can verify
/// that upstream hasn't moved between rebase and push.
#[derive(Debug, Deserialize)]
struct RebaseState {
    /// The SHA of `origin/<branch>` at the time the rebase stage fetched.
    upstream_sha: String,
}

/// Push result artifact written to `$AIRLOCK_ARTIFACTS/push_result.json`.
#[derive(Debug, Serialize)]
struct PushResult {
    success: bool,
    branch: String,
    commit_sha: String,
    upstream_url: String,
    remote_ref: String,
    output: String,
    pushed_at: i64,
}

/// Execute the `push` command.
///
/// Performs a two-phase push: updates the gate ref, then pushes from gate to upstream.
/// On push failure, reverts the gate ref so it doesn't advertise a commit upstream
/// doesn't have.
pub async fn push() -> Result<()> {
    info!("Executing push stage...");

    // Read required environment variables
    let branch_ref = std::env::var("AIRLOCK_BRANCH").context(
        "AIRLOCK_BRANCH environment variable not set. This command must be run within a pipeline stage.",
    )?;
    let upstream_url = std::env::var("AIRLOCK_UPSTREAM_URL").context(
        "AIRLOCK_UPSTREAM_URL environment variable not set. This command must be run within a pipeline stage.",
    )?;
    let worktree = PathBuf::from(std::env::var("AIRLOCK_WORKTREE").context(
        "AIRLOCK_WORKTREE environment variable not set. This command must be run within a pipeline stage.",
    )?);
    let gate_path = PathBuf::from(std::env::var("AIRLOCK_GATE_PATH").context(
        "AIRLOCK_GATE_PATH environment variable not set. This command must be run within a pipeline stage.",
    )?);
    let artifacts_dir = PathBuf::from(std::env::var("AIRLOCK_ARTIFACTS").context(
        "AIRLOCK_ARTIFACTS environment variable not set. This command must be run within a pipeline stage.",
    )?);
    // Extract branch name from full ref (e.g., "refs/heads/feature/xyz" -> "feature/xyz")
    let branch = branch_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(&branch_ref);
    let ref_name = if branch_ref.starts_with("refs/") {
        branch_ref.clone()
    } else {
        format!("refs/heads/{}", branch_ref)
    };

    println!("Pushing to upstream...");
    println!("Branch: {}", branch);
    println!("Upstream: {}", upstream_url);

    // 1. Get worktree HEAD (may include freeze commit)
    let worktree_head = git::rev_parse_head(&worktree)
        .context("Failed to get worktree HEAD. Is the worktree a valid git repo?")?;
    debug!("Worktree HEAD: {}", worktree_head);

    // 2. Sync gate from upstream
    debug!("Syncing gate from upstream...");
    if let Err(e) = git::fetch(&gate_path, "origin") {
        // Fetch failure is not fatal for first push (no upstream ref yet)
        warn!("Failed to fetch from upstream (may be first push): {}", e);
    }

    // 3. Get upstream HEAD from gate's remote tracking ref
    let tracking_ref = format!("refs/remotes/origin/{}", branch);
    let upstream_head = git::resolve_ref(&gate_path, &tracking_ref)
        .context("Failed to resolve upstream tracking ref")?;
    debug!("Upstream HEAD: {:?}", upstream_head);

    // 4. Check for divergence (if upstream ref exists)
    //
    // If the worktree HEAD isn't descended from upstream (e.g. after rebase
    // rewrote history), we need force-with-lease. We use the SHA that the
    // rebase stage recorded — NOT the freshly-fetched upstream — as the
    // lease expected value. This way force-with-lease guards the full
    // rebase→push window, not just the narrow fetch→push window.
    let lease_sha: Option<String> = if let Some(ref upstream_sha) = upstream_head {
        debug!(
            "Checking ancestry: {} -> {}",
            &upstream_sha[..8.min(upstream_sha.len())],
            &worktree_head[..8.min(worktree_head.len())]
        );

        // We need the gate to know about worktree_head for the ancestry check.
        // The worktree_head may be a freeze commit that only exists in the worktree.
        // Use the worktree for the ancestry check since it has both commits.
        let is_ancestor = git::is_ancestor_of(&worktree, upstream_sha, &worktree_head)
            .context("Failed to check commit ancestry")?;

        if !is_ancestor {
            // History diverged. Only force-push if the rebase stage ran and
            // upstream hasn't moved since it fetched. This prevents silently
            // overwriting concurrent pushes that landed between rebase and push.
            let rebase_sha = validate_rebase_state(&artifacts_dir, upstream_sha, branch)?;
            info!(
                "Upstream unchanged since rebase. \
                 Will use --force-with-lease to push safely."
            );
            Some(rebase_sha)
        } else {
            None
        }
    } else {
        None
    };

    // 5. Transfer worktree HEAD objects to gate (freeze may have created new commits)
    let worktree_str = worktree
        .to_str()
        .context("Invalid worktree path (non-UTF8)")?;
    debug!("Fetching worktree HEAD into gate...");
    if let Err(e) = git::fetch_with_refspecs(&gate_path, worktree_str, &["HEAD"]) {
        warn!("Failed to fetch worktree HEAD into gate: {}", e);
        // Fall back: try fetching the specific SHA
        let refspec = format!("{}:refs/airlock/staging", worktree_head);
        git::fetch_with_refspecs(&gate_path, worktree_str, &[&refspec])
            .context("Failed to transfer worktree commits to gate")?;
    }

    // 6. Update gate branch ref to worktree HEAD
    debug!("Updating gate ref {} to {}", ref_name, worktree_head);
    git::update_ref(&gate_path, &ref_name, &worktree_head)
        .context("Failed to update gate branch ref")?;

    // 7. Push gate to upstream
    let refspec = format!("{}:{}", ref_name, ref_name);
    let push_result = if let Some(ref expected_sha) = lease_sha {
        git::push_force_with_lease(&gate_path, "origin", &[&refspec], &ref_name, expected_sha)
    } else {
        git::push(&gate_path, "origin", &[&refspec])
    };
    match push_result {
        Ok(()) => {
            println!("Successfully pushed to {}", upstream_url);

            // Write success artifacts
            write_push_result(
                &artifacts_dir,
                true,
                branch,
                &worktree_head,
                &upstream_url,
                &ref_name,
                "Push successful",
            )?;

            info!(
                "Push complete: {} -> {} ({})",
                branch,
                upstream_url,
                &worktree_head[..12.min(worktree_head.len())]
            );

            // Best-effort: notify daemon that push succeeded so it can update
            // tracking refs and clean up protective refs.
            notify_mark_forwarded(&ref_name, &worktree_head).await;
        }
        Err(e) => {
            // Revert gate ref on push failure
            if let Some(ref original) = upstream_head {
                debug!("Reverting gate ref to {}", original);
                let _ = git::update_ref(&gate_path, &ref_name, original);
            }

            let error_msg = format!(
                "Push failed{}: {}",
                if lease_sha.is_some() {
                    " (with --force-with-lease)"
                } else {
                    ""
                },
                e
            );
            println!("{}", error_msg);

            write_push_result(
                &artifacts_dir,
                false,
                branch,
                &worktree_head,
                &upstream_url,
                &ref_name,
                &error_msg,
            )?;

            anyhow::bail!("{}", error_msg);
        }
    }

    Ok(())
}

/// Notify the daemon that a push succeeded, so it can update tracking refs
/// and clean up protective refs. Best-effort: logs a warning on failure but
/// does not fail the push stage.
async fn notify_mark_forwarded(ref_name: &str, sha: &str) {
    let run_id = match std::env::var("AIRLOCK_RUN_ID") {
        Ok(id) if !id.is_empty() => id,
        _ => {
            debug!("AIRLOCK_RUN_ID not set, skipping mark_forwarded notification");
            return;
        }
    };

    let paths = AirlockPaths::default();
    let request = ipc_client::Request::with_params(
        "mark_forwarded",
        serde_json::json!({
            "run_id": run_id,
            "ref_name": ref_name,
            "sha": sha,
        }),
    );

    match ipc_client::send_request(&paths, &request).await {
        Ok(resp) => {
            if let Some(err) = resp.error {
                warn!("mark_forwarded RPC error: {}", err.message);
            } else {
                info!("Notified daemon of successful push (mark_forwarded)");
            }
        }
        Err(e) => {
            warn!("Failed to send mark_forwarded to daemon: {}", e);
        }
    }
}

/// Check that force-pushing is safe by comparing current upstream with what the
/// rebase stage recorded. Returns the rebase-time upstream SHA (for use as the
/// force-with-lease expected value) on success, or bails with a clear error if
/// upstream moved or no rebase state exists.
fn validate_rebase_state(
    artifacts_dir: &Path,
    current_upstream_sha: &str,
    branch: &str,
) -> Result<String> {
    let state_path = artifacts_dir.join("rebase_state.json");

    let state_json = match std::fs::read_to_string(&state_path) {
        Ok(json) => json,
        Err(_) => {
            anyhow::bail!(
                "Push rejected: history has diverged from upstream but no rebase state found.\n\
                 The rebase stage must run before push to handle diverged history safely."
            );
        }
    };

    let state: RebaseState =
        serde_json::from_str(&state_json).context("Failed to parse rebase_state.json")?;

    if state.upstream_sha != current_upstream_sha {
        anyhow::bail!(
            "Push rejected: upstream '{}' changed since the rebase stage ran.\n\
             Rebase saw: {}\n\
             Upstream now: {}\n\n\
             Someone pushed to this branch while the pipeline was running.\n\
             The push has been aborted to avoid overwriting their changes.",
            branch,
            &state.upstream_sha[..12.min(state.upstream_sha.len())],
            &current_upstream_sha[..12.min(current_upstream_sha.len())]
        );
    }

    debug!(
        "Rebase state validated: upstream {} unchanged since rebase",
        &current_upstream_sha[..8.min(current_upstream_sha.len())]
    );
    Ok(state.upstream_sha)
}

/// Write `push_result.json` to the artifacts directory.
fn write_push_result(
    artifacts_dir: &Path,
    success: bool,
    branch: &str,
    commit_sha: &str,
    upstream_url: &str,
    remote_ref: &str,
    output: &str,
) -> Result<()> {
    std::fs::create_dir_all(artifacts_dir)
        .with_context(|| format!("Failed to create artifacts directory: {:?}", artifacts_dir))?;

    let result = PushResult {
        success,
        branch: branch.to_string(),
        commit_sha: commit_sha.to_string(),
        upstream_url: upstream_url.to_string(),
        remote_ref: remote_ref.to_string(),
        output: output.to_string(),
        pushed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
    };

    let json =
        serde_json::to_string_pretty(&result).context("Failed to serialize push result to JSON")?;

    let path = artifacts_dir.join("push_result.json");
    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write push_result.json: {:?}", path))?;

    debug!("Wrote push_result.json to {:?}", path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::process::Command;
    use tempfile::TempDir;

    /// Helper to create a git repo with an initial commit.
    fn setup_git_repo(path: &std::path::Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(path)
            .output()
            .unwrap();
        std::fs::write(path.join("file.txt"), "initial\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(path)
            .output()
            .unwrap();
    }

    /// Helper to get HEAD SHA.
    fn get_head(path: &std::path::Path) -> String {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(path)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Sets up the standard test topology: upstream, gate, worktree.
    /// Returns (upstream_path, gate_path, worktree_path, work_path).
    fn setup_push_topology(temp_dir: &TempDir) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        // Create "upstream" bare repo
        let upstream_path = temp_dir.path().join("upstream.git");
        Command::new("git")
            .args(["init", "--bare"])
            .arg(&upstream_path)
            .output()
            .unwrap();

        // Create a working repo and push initial commit to upstream
        let work_path = temp_dir.path().join("work");
        std::fs::create_dir_all(&work_path).unwrap();
        setup_git_repo(&work_path);
        Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(&work_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["remote", "add", "origin"])
            .arg(upstream_path.to_str().unwrap())
            .current_dir(&work_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["push", "-u", "origin", "main"])
            .current_dir(&work_path)
            .output()
            .unwrap();

        // Ensure upstream HEAD points to main
        Command::new("git")
            .args(["symbolic-ref", "HEAD", "refs/heads/main"])
            .current_dir(&upstream_path)
            .output()
            .unwrap();

        // Create gate bare repo with upstream as origin
        let gate_path = temp_dir.path().join("gate.git");
        Command::new("git")
            .args(["init", "--bare"])
            .arg(&gate_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["remote", "add", "origin"])
            .arg(upstream_path.to_str().unwrap())
            .current_dir(&gate_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["fetch", "origin"])
            .current_dir(&gate_path)
            .output()
            .unwrap();

        // Create worktree by cloning from upstream
        let worktree_path = temp_dir.path().join("worktree");
        Command::new("git")
            .args(["clone"])
            .arg(upstream_path.to_str().unwrap())
            .arg(&worktree_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&worktree_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&worktree_path)
            .output()
            .unwrap();

        (upstream_path, gate_path, worktree_path, work_path)
    }

    /// Set all AIRLOCK_* env vars for the push command.
    fn set_push_env(
        upstream_path: &Path,
        gate_path: &Path,
        worktree_path: &Path,
        work_path: &Path,
        artifacts_dir: &Path,
    ) {
        let worktree_head = get_head(worktree_path);
        std::env::set_var("AIRLOCK_BRANCH", "refs/heads/main");
        std::env::set_var("AIRLOCK_UPSTREAM_URL", upstream_path.to_str().unwrap());
        std::env::set_var("AIRLOCK_WORKTREE", worktree_path.to_str().unwrap());
        std::env::set_var("AIRLOCK_GATE_PATH", gate_path.to_str().unwrap());
        std::env::set_var("AIRLOCK_ARTIFACTS", artifacts_dir.to_str().unwrap());
        std::env::set_var("AIRLOCK_HEAD_SHA", &worktree_head);
        std::env::set_var("AIRLOCK_REPO_ROOT", work_path.to_str().unwrap());
    }

    fn clear_push_env() {
        for var in [
            "AIRLOCK_BRANCH",
            "AIRLOCK_UPSTREAM_URL",
            "AIRLOCK_WORKTREE",
            "AIRLOCK_GATE_PATH",
            "AIRLOCK_ARTIFACTS",
            "AIRLOCK_HEAD_SHA",
            "AIRLOCK_REPO_ROOT",
        ] {
            std::env::remove_var(var);
        }
    }

    /// After freeze creates a commit, push should update the gate ref AND upstream.
    #[tokio::test]
    #[serial]
    async fn test_push_with_freeze_commit_updates_gate_and_upstream() {
        let temp_dir = TempDir::new().unwrap();
        let (upstream_path, gate_path, worktree_path, work_path) = setup_push_topology(&temp_dir);

        // Simulate freeze: add a commit to the worktree
        std::fs::write(worktree_path.join("fix.txt"), "auto-fix\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(&worktree_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "Airlock: auto-fix"])
            .current_dir(&worktree_path)
            .output()
            .unwrap();

        let worktree_head = get_head(&worktree_path);

        let artifacts_dir = temp_dir.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        set_push_env(
            &upstream_path,
            &gate_path,
            &worktree_path,
            &work_path,
            &artifacts_dir,
        );

        push().await.unwrap();

        // Gate ref must match the freeze commit
        let gate_ref = git::resolve_ref(&gate_path, "refs/heads/main")
            .unwrap()
            .expect("gate should have refs/heads/main");
        assert_eq!(
            gate_ref, worktree_head,
            "Gate ref should match worktree HEAD (the freeze commit)"
        );

        // Upstream must also match
        let upstream_head = get_head(&upstream_path);
        assert_eq!(
            upstream_head, worktree_head,
            "Upstream should match worktree HEAD"
        );

        // push_result.json should record the freeze commit SHA, not the stale AIRLOCK_HEAD_SHA
        let result: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(artifacts_dir.join("push_result.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["commit_sha"], worktree_head);

        clear_push_env();
    }

    /// Write a rebase_state.json artifact with the given upstream SHA.
    fn write_rebase_state(artifacts_dir: &Path, upstream_sha: &str) {
        std::fs::create_dir_all(artifacts_dir).unwrap();
        let state = format!(r#"{{"upstream_sha":"{}"}}"#, upstream_sha);
        std::fs::write(artifacts_dir.join("rebase_state.json"), state).unwrap();
    }

    /// After rebase rewrites history (worktree HEAD is not a descendant of
    /// upstream), push should succeed via --force-with-lease when rebase_state
    /// confirms upstream hasn't moved.
    #[tokio::test]
    #[serial]
    async fn test_push_succeeds_after_rebase_rewrite() {
        let temp_dir = TempDir::new().unwrap();
        let (upstream_path, gate_path, worktree_path, work_path) = setup_push_topology(&temp_dir);

        // Record upstream SHA (what rebase would have seen after fetching)
        let upstream_sha_at_rebase = get_head(&upstream_path);

        // Simulate rebase rewriting history: create a NEW commit in worktree that
        // is NOT a descendant of the current upstream HEAD (orphan-style).
        // We do this by amending the initial commit, which changes its SHA.
        std::fs::write(worktree_path.join("file.txt"), "rewritten by rebase\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(&worktree_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--amend", "-m", "initial (rebased)"])
            .current_dir(&worktree_path)
            .output()
            .unwrap();

        let worktree_head = get_head(&worktree_path);

        let artifacts_dir = temp_dir.path().join("artifacts");
        write_rebase_state(&artifacts_dir, &upstream_sha_at_rebase);
        set_push_env(
            &upstream_path,
            &gate_path,
            &worktree_path,
            &work_path,
            &artifacts_dir,
        );

        // Push should succeed via force-with-lease
        push()
            .await
            .expect("Push should succeed after rebase rewrite");

        // Upstream should now match the rewritten worktree HEAD
        let upstream_head = get_head(&upstream_path);
        assert_eq!(
            upstream_head, worktree_head,
            "Upstream should match rewritten worktree HEAD"
        );

        // push_result.json should record success
        let result: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(artifacts_dir.join("push_result.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(result["success"], true);
        assert_eq!(result["commit_sha"], worktree_head);

        clear_push_env();
    }

    /// When someone pushes to upstream after rebase ran, push must fail because
    /// the rebase_state.json SHA won't match the current upstream.
    #[tokio::test]
    #[serial]
    async fn test_push_fails_when_upstream_moved_since_rebase() {
        let temp_dir = TempDir::new().unwrap();
        let (upstream_path, gate_path, worktree_path, work_path) = setup_push_topology(&temp_dir);

        // Record the upstream SHA that rebase would have seen
        let upstream_sha_at_rebase = get_head(&upstream_path);

        // Someone pushes a concurrent commit to upstream AFTER rebase ran
        std::fs::write(work_path.join("other.txt"), "concurrent\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(&work_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "concurrent commit"])
            .current_dir(&work_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["push", "origin", "main"])
            .current_dir(&work_path)
            .output()
            .unwrap();

        // Rewrite worktree history (simulate rebase on old upstream)
        std::fs::write(worktree_path.join("file.txt"), "rebased\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(&worktree_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--amend", "-m", "rebased"])
            .current_dir(&worktree_path)
            .output()
            .unwrap();

        let artifacts_dir = temp_dir.path().join("artifacts");
        // rebase_state records the OLD upstream SHA (before concurrent push)
        write_rebase_state(&artifacts_dir, &upstream_sha_at_rebase);
        set_push_env(
            &upstream_path,
            &gate_path,
            &worktree_path,
            &work_path,
            &artifacts_dir,
        );

        // Push should fail: upstream moved since rebase
        let result = push().await;
        assert!(
            result.is_err(),
            "Push should fail when upstream moved since rebase"
        );

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("changed since the rebase stage ran"),
            "Error should explain upstream moved, got: {}",
            err_msg
        );

        clear_push_env();
    }

    /// When history diverges but no rebase_state.json exists, push must fail
    /// with a clear message about missing rebase state.
    #[tokio::test]
    #[serial]
    async fn test_push_fails_on_divergence_without_rebase_state() {
        let temp_dir = TempDir::new().unwrap();
        let (upstream_path, gate_path, worktree_path, work_path) = setup_push_topology(&temp_dir);

        // Rewrite worktree history without writing rebase_state.json
        std::fs::write(worktree_path.join("file.txt"), "diverged\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(&worktree_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--amend", "-m", "diverged"])
            .current_dir(&worktree_path)
            .output()
            .unwrap();

        let artifacts_dir = temp_dir.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        // Deliberately NOT writing rebase_state.json
        set_push_env(
            &upstream_path,
            &gate_path,
            &worktree_path,
            &work_path,
            &artifacts_dir,
        );

        let result = push().await;
        assert!(result.is_err(), "Push should fail without rebase state");

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("no rebase state found"),
            "Error should explain missing rebase state, got: {}",
            err_msg
        );

        clear_push_env();
    }

    /// Helper to resolve a specific ref (not just HEAD).
    fn resolve_ref(path: &std::path::Path, refspec: &str) -> String {
        let output = Command::new("git")
            .args(["rev-parse", refspec])
            .current_dir(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Failed to resolve {}: {}",
            refspec,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Push with force-with-lease on a feature branch in a detached worktree.
    ///
    /// Uses production-like topology: gate bare repo → detached worktree,
    /// pushing to a feature branch (not main) with diverged history.
    /// The rebase stage's correct recording of rebase_state.json is validated
    /// by the executor-level E2E test in airlock-daemon; this test validates
    /// that the push stage correctly consumes it.
    #[tokio::test]
    #[serial]
    async fn test_push_force_with_lease_on_feature_branch_detached_worktree() {
        let temp_dir = TempDir::new().unwrap();

        // --- Upstream bare repo ---
        let upstream_path = temp_dir.path().join("upstream.git");
        Command::new("git")
            .args(["init", "--bare"])
            .arg(&upstream_path)
            .output()
            .unwrap();

        // --- Working repo: push initial commit to main ---
        let work_path = temp_dir.path().join("work");
        std::fs::create_dir_all(&work_path).unwrap();
        setup_git_repo(&work_path);
        Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(&work_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["remote", "add", "origin"])
            .arg(upstream_path.to_str().unwrap())
            .current_dir(&work_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["push", "-u", "origin", "main"])
            .current_dir(&work_path)
            .output()
            .unwrap();

        // --- Create feature branch with a different commit ---
        Command::new("git")
            .args(["checkout", "-b", "feat/test-branch"])
            .current_dir(&work_path)
            .output()
            .unwrap();
        std::fs::write(work_path.join("feature.txt"), "feature work\n").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(&work_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "feature commit"])
            .current_dir(&work_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["push", "-u", "origin", "feat/test-branch"])
            .current_dir(&work_path)
            .output()
            .unwrap();

        let feature_sha = resolve_ref(&upstream_path, "refs/heads/feat/test-branch");

        // --- Gate bare repo ---
        let gate_path = temp_dir.path().join("gate.git");
        Command::new("git")
            .args(["init", "--bare"])
            .arg(&gate_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["remote", "add", "origin"])
            .arg(upstream_path.to_str().unwrap())
            .current_dir(&gate_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["fetch", "origin"])
            .current_dir(&gate_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&gate_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&gate_path)
            .output()
            .unwrap();

        // --- Detached worktree from gate (matching production) ---
        let worktree_path = temp_dir.path().join("worktree");
        let wt_result = Command::new("git")
            .args([
                "worktree",
                "add",
                "--detach",
                worktree_path.to_str().unwrap(),
                &feature_sha,
            ])
            .current_dir(&gate_path)
            .output()
            .unwrap();
        assert!(
            wt_result.status.success(),
            "worktree add failed: {}",
            String::from_utf8_lossy(&wt_result.stderr)
        );

        // --- Simulate rebase: amend the commit (diverged history) ---
        std::fs::write(
            worktree_path.join("feature.txt"),
            "feature work (rebased)\n",
        )
        .unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(&worktree_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--amend", "-m", "feature commit (rebased)"])
            .current_dir(&worktree_path)
            .output()
            .unwrap();
        let worktree_head = get_head(&worktree_path);

        // --- Write rebase_state with correct feature branch SHA ---
        let artifacts_dir = temp_dir.path().join("artifacts");
        write_rebase_state(&artifacts_dir, &feature_sha);

        // --- Run push ---
        std::env::set_var("AIRLOCK_BRANCH", "refs/heads/feat/test-branch");
        std::env::set_var("AIRLOCK_UPSTREAM_URL", upstream_path.to_str().unwrap());
        std::env::set_var("AIRLOCK_WORKTREE", worktree_path.to_str().unwrap());
        std::env::set_var("AIRLOCK_GATE_PATH", gate_path.to_str().unwrap());
        std::env::set_var("AIRLOCK_ARTIFACTS", artifacts_dir.to_str().unwrap());
        std::env::set_var("AIRLOCK_HEAD_SHA", &worktree_head);
        std::env::set_var("AIRLOCK_REPO_ROOT", work_path.to_str().unwrap());

        push()
            .await
            .expect("Push should succeed with force-with-lease on feature branch");

        // Upstream should now have the rebased commit
        let new_upstream = resolve_ref(&upstream_path, "refs/heads/feat/test-branch");
        assert_eq!(
            new_upstream, worktree_head,
            "Upstream feature branch should have the rebased commit"
        );

        clear_push_env();
    }

    /// Push without a freeze commit (worktree HEAD == original push) should still
    /// update the gate ref and succeed.
    #[tokio::test]
    #[serial]
    async fn test_push_without_freeze_commit_syncs_gate() {
        let temp_dir = TempDir::new().unwrap();
        let (upstream_path, gate_path, worktree_path, work_path) = setup_push_topology(&temp_dir);

        // No freeze commit — worktree HEAD is the original commit
        let worktree_head = get_head(&worktree_path);

        let artifacts_dir = temp_dir.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        set_push_env(
            &upstream_path,
            &gate_path,
            &worktree_path,
            &work_path,
            &artifacts_dir,
        );

        push().await.unwrap();

        // Gate ref should be set
        let gate_ref = git::resolve_ref(&gate_path, "refs/heads/main")
            .unwrap()
            .expect("gate should have refs/heads/main");
        assert_eq!(gate_ref, worktree_head);

        clear_push_env();
    }
}
