//! Fetch operations for Airlock (CLI-based for proper credential support).

use super::cmd::{run_git, run_git_network, run_git_unchecked};
use crate::error::{AirlockError, Result};
use std::path::Path;
use std::process::Command;

/// Fetch from a remote using git CLI.
///
/// This uses the git CLI instead of libgit2 to properly support
/// SSH agents, credential helpers, and other authentication methods.
pub fn fetch(repo_path: &Path, remote_name: &str) -> Result<()> {
    tracing::debug!("Fetching from '{}' in {}", remote_name, repo_path.display());

    run_git_network(repo_path, &["fetch", remote_name], "fetch")?;

    tracing::debug!("Fetched from remote '{}'", remote_name);
    Ok(())
}

/// Fetch specific refspecs from a remote using git CLI.
pub fn fetch_with_refspecs(repo_path: &Path, remote_name: &str, refspecs: &[&str]) -> Result<()> {
    tracing::debug!(
        "Fetching refspecs {:?} from '{}' in {}",
        refspecs,
        remote_name,
        repo_path.display()
    );

    let mut args: Vec<&str> = vec!["fetch", remote_name];
    args.extend_from_slice(refspecs);
    run_git_network(repo_path, &args, "fetch")?;

    tracing::debug!("Fetched from remote '{}'", remote_name);
    Ok(())
}

/// Fetch all refs from a remote using git CLI, pruning deleted refs.
pub fn fetch_all(repo_path: &Path, remote_name: &str) -> Result<()> {
    tracing::debug!(
        "Fetching all from '{}' in {}",
        remote_name,
        repo_path.display()
    );

    run_git_network(repo_path, &["fetch", "--prune", remote_name], "fetch")?;

    Ok(())
}

/// Create local tracking branches for remote branches that don't have local counterparts.
///
/// This is useful after `airlock init` to ensure that tools like jj can resolve
/// branch names (e.g., `main`) instead of requiring the remote syntax (`main@origin`).
///
/// For each remote-tracking branch `refs/remotes/<remote>/<branch>`, if there is no
/// corresponding `refs/heads/<branch>`, this creates the local branch pointing to
/// the same commit.
///
/// This uses the git CLI for consistency with other git operations.
pub fn create_local_tracking_branches(repo_path: &Path, remote_name: &str) -> Result<()> {
    tracing::debug!(
        "Creating local tracking branches from '{}' in {}",
        remote_name,
        repo_path.display()
    );

    // Get list of remote branches
    let ref_pattern = format!("refs/remotes/{}/", remote_name);
    let output = run_git(
        repo_path,
        &["for-each-ref", "--format=%(refname:short)", &ref_pattern],
        "list remote branches",
    )?;

    let remote_branches = String::from_utf8_lossy(&output.stdout);
    let prefix = format!("{}/", remote_name);

    for line in remote_branches.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Extract branch name (e.g., "origin/main" -> "main")
        let branch_name = match line.strip_prefix(&prefix) {
            Some(name) => name,
            None => continue,
        };

        // Skip HEAD reference
        if branch_name == "HEAD" {
            continue;
        }

        // Check if local branch already exists
        let check_ref = format!("refs/heads/{}", branch_name);
        let check_output = run_git_unchecked(
            repo_path,
            &["show-ref", "--verify", "--quiet", &check_ref],
            "show-ref",
        )?;

        if check_output.status.success() {
            // Local branch already exists, skip
            tracing::debug!("Local branch '{}' already exists, skipping", branch_name);
            continue;
        }

        // Create local branch tracking the remote
        tracing::debug!(
            "Creating local branch '{}' tracking '{}/{}'",
            branch_name,
            remote_name,
            branch_name
        );

        let tracking_ref = format!("{}/{}", remote_name, branch_name);
        let create_output = run_git_unchecked(
            repo_path,
            &["branch", "--track", branch_name, &tracking_ref],
            "branch --track",
        )?;

        if !create_output.status.success() {
            let stderr = String::from_utf8_lossy(&create_output.stderr);
            // Log warning but don't fail - some branches might have issues
            tracing::warn!(
                "Failed to create local branch '{}': {}",
                branch_name,
                stderr.trim()
            );
        }
    }

    tracing::debug!("Finished creating local tracking branches");
    Ok(())
}

/// Ensure all local branches have tracking set for the given remote.
///
/// For each local branch that either has no upstream or tracks a different remote,
/// if a matching `refs/remotes/<remote_name>/<branch>` ref exists, set it as the
/// upstream. This is a safety net that runs after other tracking operations to
/// catch branches that lost tracking (e.g., due to `remote_delete` stripping
/// `branch.*.remote` config entries).
pub fn ensure_tracking_for_existing_branches(repo_path: &Path, remote_name: &str) -> Result<()> {
    tracing::debug!(
        "Ensuring tracking for existing branches against '{}' in {}",
        remote_name,
        repo_path.display()
    );

    // List all local branches with their current tracking remote
    let output = run_git(
        repo_path,
        &[
            "for-each-ref",
            "--format=%(refname:short) %(upstream:remotename)",
            "refs/heads/",
        ],
        "list branches",
    )?;

    let branch_info = String::from_utf8_lossy(&output.stdout);

    for line in branch_info.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        let branch_name = parts[0];
        let tracking_remote = if parts.len() == 2 { parts[1] } else { "" };

        // Skip branches that already track the correct remote
        if tracking_remote == remote_name {
            continue;
        }

        // Check if a matching remote-tracking ref exists
        let remote_ref = format!("refs/remotes/{}/{}", remote_name, branch_name);
        let check_output = run_git_unchecked(
            repo_path,
            &["show-ref", "--verify", "--quiet", &remote_ref],
            "show-ref",
        )?;

        if !check_output.status.success() {
            // No matching remote ref, skip
            continue;
        }

        // Set upstream tracking
        let upstream_ref = format!("{}/{}", remote_name, branch_name);
        let set_output = run_git_unchecked(
            repo_path,
            &["branch", "--set-upstream-to", &upstream_ref, branch_name],
            "branch --set-upstream-to",
        )?;

        if set_output.status.success() {
            tracing::debug!(
                "Set tracking for branch '{}' to '{}/{}'",
                branch_name,
                remote_name,
                branch_name
            );
        } else {
            let stderr = String::from_utf8_lossy(&set_output.stderr);
            tracing::warn!(
                "Failed to set tracking for branch '{}': {}",
                branch_name,
                stderr.trim()
            );
        }
    }

    tracing::debug!("Finished ensuring tracking for existing branches");
    Ok(())
}

/// Mirror refs from a remote directly into local refs.
///
/// This is used for the gate repository to mirror upstream branches and tags
/// so that clients fetching from the gate can see them. Unlike a normal fetch
/// which creates refs under `refs/remotes/<remote>/*`, this mirrors directly
/// to `refs/heads/*` and `refs/tags/*`.
///
/// This uses the git CLI instead of libgit2 to properly support
/// SSH agents, credential helpers, and other authentication methods.
pub fn mirror_from_remote(repo_path: &Path, remote_name: &str) -> Result<()> {
    tracing::debug!(
        "Mirroring from '{}' into {}",
        remote_name,
        repo_path.display()
    );

    // Fetch branches directly into refs/heads/*
    // The + prefix forces the update even if it's not a fast-forward
    run_git_network(
        repo_path,
        &[
            "fetch",
            "--prune",
            remote_name,
            "+refs/heads/*:refs/heads/*",
            "+refs/tags/*:refs/tags/*",
        ],
        "fetch",
    )?;

    tracing::debug!("Mirrored from remote '{}'", remote_name);
    Ok(())
}

/// Controls how rebase conflicts are handled during smart sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolver {
    /// Abort rebase immediately on conflicts (default for tests/non-daemon contexts).
    Abort,
    /// Invoke `airlock exec agent` to resolve conflicts.
    Agent,
}

/// Status of a single branch after smart sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchSyncStatus {
    /// Branch was created (only existed on remote).
    Created,
    /// Branch was already up-to-date.
    UpToDate,
    /// Branch was fast-forwarded to match remote.
    FastForwarded,
    /// Gate is ahead of remote — no action taken.
    GateAhead,
    /// Branch was rebased on top of upstream (diverged, clean rebase).
    Rebased,
    /// Rebase had conflicts that were resolved by the agent.
    RebasedWithConflictResolution,
    /// Rebase failed — branch left as-is.
    RebaseFailed { reason: String },
}

/// Report from a smart sync operation.
#[derive(Debug, Clone)]
pub struct SyncReport {
    /// Per-branch sync results.
    pub branches: Vec<(String, BranchSyncStatus)>,
    /// Branches that had conflicts the agent could not resolve.
    pub warnings: Vec<String>,
}

impl SyncReport {
    fn new() -> Self {
        Self {
            branches: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

/// List local branch names in a repository.
pub fn list_local_branches(repo_path: &Path) -> Result<Vec<String>> {
    let output = run_git(
        repo_path,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads/"],
        "list branches",
    )?;

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// List remote-tracking branch names for a given remote.
fn list_remote_branches(repo_path: &Path, remote_name: &str) -> Result<Vec<String>> {
    let ref_pattern = format!("refs/remotes/{}/", remote_name);
    let output = run_git(
        repo_path,
        &["for-each-ref", "--format=%(refname:short)", &ref_pattern],
        "list remote branches",
    )?;

    let prefix = format!("{}/", remote_name);
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.strip_prefix(&prefix).map(|b| b.to_string()))
        .filter(|s| s != "HEAD")
        .collect())
}

/// Smart sync from a remote, preserving un-forwarded local commits.
///
/// Unlike `mirror_from_remote` which force-overwrites local branches,
/// this function:
/// 1. Fetches into `refs/remotes/{remote}/*` (standard fetch)
/// 2. Force-updates tags
/// 3. For each branch, intelligently merges:
///    - New remote branches → create locally
///    - Same SHA → skip
///    - Gate behind (fast-forward) → update
///    - Gate ahead → skip (already a superset)
///    - Diverged → rebase local commits on upstream in a temporary worktree
///
/// The `sync_worktree_dir` is a directory where temporary worktrees will be
/// created for rebase operations. If `None`, diverged branches will be left
/// as-is with a warning.
pub fn smart_sync_from_remote(
    repo_path: &Path,
    remote_name: &str,
    sync_worktree_dir: Option<&Path>,
    conflict_resolver: ConflictResolver,
) -> Result<SyncReport> {
    tracing::debug!(
        "Smart syncing from '{}' in {}",
        remote_name,
        repo_path.display()
    );

    let mut report = SyncReport::new();

    // Step 1: Standard fetch with prune into refs/remotes/{remote}/*
    fetch_all(repo_path, remote_name)?;

    // Step 2: Force-update tags
    let tag_result = fetch_with_refspecs(repo_path, remote_name, &["+refs/tags/*:refs/tags/*"]);
    if let Err(e) = tag_result {
        tracing::warn!("Failed to sync tags: {}", e);
    }

    // Step 3: Compare and sync each branch
    let remote_branches = list_remote_branches(repo_path, remote_name)?;
    let local_branches = list_local_branches(repo_path)?;

    // Process branches that exist on the remote
    for branch in &remote_branches {
        let local_ref = format!("refs/heads/{}", branch);
        let remote_ref = format!("refs/remotes/{}/{}", remote_name, branch);

        let remote_sha = match super::refs::resolve_ref(repo_path, &remote_ref)? {
            Some(sha) => sha,
            None => continue,
        };

        let local_sha = super::refs::resolve_ref(repo_path, &local_ref)?;

        match local_sha {
            None => {
                // Branch only exists on remote — create it
                super::refs::update_ref(repo_path, &local_ref, &remote_sha)?;
                report
                    .branches
                    .push((branch.clone(), BranchSyncStatus::Created));
                tracing::debug!("Created local branch '{}' from remote", branch);
            }
            Some(local_sha) if local_sha == remote_sha => {
                report
                    .branches
                    .push((branch.clone(), BranchSyncStatus::UpToDate));
            }
            Some(local_sha) => {
                // Check ancestry
                let local_is_ancestor =
                    super::refs::is_ancestor_of(repo_path, &local_sha, &remote_sha)?;
                let remote_is_ancestor =
                    super::refs::is_ancestor_of(repo_path, &remote_sha, &local_sha)?;

                if local_is_ancestor {
                    // Gate behind — fast-forward
                    super::refs::update_ref(repo_path, &local_ref, &remote_sha)?;
                    report
                        .branches
                        .push((branch.clone(), BranchSyncStatus::FastForwarded));
                    tracing::debug!("Fast-forwarded branch '{}'", branch);
                } else if remote_is_ancestor {
                    // Gate ahead — skip (already a superset of upstream)
                    report
                        .branches
                        .push((branch.clone(), BranchSyncStatus::GateAhead));
                    tracing::debug!(
                        "Branch '{}' is ahead of remote, skipping (gate is superset)",
                        branch
                    );
                } else {
                    // Diverged — attempt rebase in temporary worktree
                    let status = rebase_diverged_branch(
                        repo_path,
                        branch,
                        &remote_ref,
                        sync_worktree_dir,
                        conflict_resolver,
                    );
                    if let BranchSyncStatus::RebaseFailed { reason } = &status {
                        let warning = format!(
                            "Branch '{}' has diverged from upstream and auto-rebase failed: {}.\n\
                             To resolve manually:\n  \
                             git fetch upstream\n  \
                             git rebase upstream/{}\n  \
                             # resolve conflicts\n  \
                             git push origin {}",
                            branch, reason, branch, branch
                        );
                        report.warnings.push(warning);
                    }
                    report.branches.push((branch.clone(), status));
                }
            }
        }
    }

    // Prune local branches that no longer exist on remote
    // (Only prune branches that were previously tracking this remote)
    for branch in &local_branches {
        if !remote_branches.contains(branch) {
            // Check if this branch was tracking the remote
            let remote_ref = format!("refs/remotes/{}/{}", remote_name, branch);
            if super::refs::resolve_ref(repo_path, &remote_ref)?.is_none() {
                // Remote-tracking ref doesn't exist — this branch was deleted upstream
                // We don't auto-delete local branches that have un-forwarded commits,
                // just log it
                tracing::debug!(
                    "Branch '{}' no longer on remote (may have been deleted upstream)",
                    branch
                );
            }
        }
    }

    tracing::debug!(
        "Smart sync complete: {} branches processed, {} warnings",
        report.branches.len(),
        report.warnings.len()
    );

    Ok(report)
}

/// Rebase a diverged local branch on top of the remote ref using a temporary worktree.
///
/// Returns the sync status for the branch.
fn rebase_diverged_branch(
    repo_path: &Path,
    branch: &str,
    remote_ref: &str,
    sync_worktree_dir: Option<&Path>,
    conflict_resolver: ConflictResolver,
) -> BranchSyncStatus {
    let sync_dir = match sync_worktree_dir {
        Some(dir) => dir,
        None => {
            return BranchSyncStatus::RebaseFailed {
                reason: "No sync worktree directory configured".to_string(),
            };
        }
    };

    // Create a sanitized directory name from the branch
    let safe_name = branch.replace('/', "_");
    let worktree_path = sync_dir.join(&safe_name);

    tracing::debug!(
        "Rebasing diverged branch '{}' in worktree {}",
        branch,
        worktree_path.display()
    );

    // Ensure parent directory exists
    if let Err(e) = std::fs::create_dir_all(sync_dir) {
        return BranchSyncStatus::RebaseFailed {
            reason: format!("Failed to create sync worktree directory: {}", e),
        };
    }

    // Clean up any stale worktree at this path
    if worktree_path.exists() {
        let _ = remove_worktree(repo_path, &worktree_path);
    }

    // Create temporary worktree for the branch
    let add_output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["worktree", "add"])
        .arg(&worktree_path)
        .arg(branch)
        .output();

    let add_output = match add_output {
        Ok(o) => o,
        Err(e) => {
            return BranchSyncStatus::RebaseFailed {
                reason: format!("Failed to create worktree: {}", e),
            };
        }
    };

    if !add_output.status.success() {
        let stderr = String::from_utf8_lossy(&add_output.stderr);
        return BranchSyncStatus::RebaseFailed {
            reason: format!("Failed to create worktree: {}", stderr.trim()),
        };
    }

    // Attempt rebase
    let rebase_output = Command::new("git")
        .arg("-C")
        .arg(&worktree_path)
        .args(["rebase", remote_ref])
        .output();

    let status = match rebase_output {
        Ok(output) if output.status.success() => {
            tracing::debug!("Clean rebase of branch '{}' onto upstream", branch);
            BranchSyncStatus::Rebased
        }
        Ok(_) => {
            // Rebase had conflicts
            match conflict_resolver {
                ConflictResolver::Abort => {
                    tracing::warn!(
                        "Rebase of branch '{}' had conflicts, aborting (no agent)",
                        branch
                    );
                    let _ = Command::new("git")
                        .arg("-C")
                        .arg(&worktree_path)
                        .args(["rebase", "--abort"])
                        .output();
                    BranchSyncStatus::RebaseFailed {
                        reason: "Rebase conflicts (agent resolution not configured)".to_string(),
                    }
                }
                ConflictResolver::Agent => {
                    tracing::info!(
                        "Rebase of branch '{}' had conflicts, invoking agent",
                        branch
                    );
                    match invoke_agent_for_rebase_conflicts(&worktree_path) {
                        Ok(()) => {
                            tracing::info!(
                                "Agent resolved rebase conflicts for branch '{}'",
                                branch
                            );
                            BranchSyncStatus::RebasedWithConflictResolution
                        }
                        Err(reason) => {
                            tracing::warn!(
                                "Agent failed to resolve conflicts for branch '{}': {}",
                                branch,
                                reason
                            );
                            let _ = Command::new("git")
                                .arg("-C")
                                .arg(&worktree_path)
                                .args(["rebase", "--abort"])
                                .output();
                            BranchSyncStatus::RebaseFailed { reason }
                        }
                    }
                }
            }
        }
        Err(e) => BranchSyncStatus::RebaseFailed {
            reason: format!("Failed to execute git rebase: {}", e),
        },
    };

    // Clean up worktree
    let _ = remove_worktree(repo_path, &worktree_path);

    status
}

/// Invoke the agent to resolve rebase conflicts in a worktree.
///
/// Shells out to `airlock exec agent` with a prompt describing the conflicts
/// and a JSON schema for structured output. Returns `Ok(())` if the agent
/// reports "pass" (conflicts resolved), or `Err(reason)` otherwise.
fn invoke_agent_for_rebase_conflicts(worktree_path: &Path) -> std::result::Result<(), String> {
    // Get the list of conflicted files
    let conflict_output = Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .args(["diff", "--name-only", "--diff-filter=U"])
        .output()
        .map_err(|e| format!("Failed to list conflicted files: {}", e))?;

    let conflicted_files = String::from_utf8_lossy(&conflict_output.stdout)
        .trim()
        .to_string();

    if conflicted_files.is_empty() {
        return Err("No conflicted files found but rebase failed".to_string());
    }

    let prompt = format!(
        "You are resolving merge conflicts during a git rebase operation.\n\
         \n\
         The local branch is being rebased onto upstream changes.\n\
         Local changes are the user's un-forwarded commits; incoming changes are from upstream.\n\
         \n\
         ## Conflicted Files\n\
         {conflicted_files}\n\
         \n\
         ## Your Task\n\
         \n\
         1. Read each conflicted file and understand the conflict markers\n\
         2. Resolve each conflict by choosing the correct resolution that preserves both the local intent and upstream changes\n\
         3. After resolving each file, run `git add <file>` to mark it resolved\n\
         4. After all conflicts are resolved, run `git rebase --continue`\n\
         5. If further conflicts arise during continue, resolve those too\n\
         \n\
         ## Important Notes\n\
         - Prefer preserving both sets of changes when possible\n\
         - If changes are incompatible, prefer the upstream version but preserve local intent\n\
         - Do NOT run `git rebase --abort` — we want to complete the rebase\n\
         \n\
         ## Final Output\n\
         When done, respond with JSON:\n\
         {{\n\
           \"verdict\": \"pass\" | \"fail\",\n\
           \"summary\": \"Brief description of how conflicts were resolved\",\n\
           \"files_resolved\": [\"list\", \"of\", \"resolved\", \"files\"]\n\
         }}"
    );

    let schema = r#"{"type":"object","properties":{"verdict":{"type":"string","enum":["pass","fail"]},"summary":{"type":"string"},"files_resolved":{"type":"array","items":{"type":"string"}}},"required":["verdict","summary"]}"#;

    tracing::debug!(
        "Invoking agent for conflict resolution in {}",
        worktree_path.display()
    );

    let agent_output = Command::new("airlock")
        .args(["exec", "agent", &prompt, "--output-schema", schema])
        .current_dir(worktree_path)
        .output()
        .map_err(|e| format!("Failed to invoke agent: {}", e))?;

    if !agent_output.status.success() {
        let stderr = String::from_utf8_lossy(&agent_output.stderr);
        return Err(format!("Agent exited with error: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&agent_output.stdout);
    let response: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("Failed to parse agent response: {}", e))?;

    match response.get("verdict").and_then(|v| v.as_str()) {
        Some("pass") => {
            tracing::info!(
                "Agent resolved conflicts: {}",
                response
                    .get("summary")
                    .and_then(|s| s.as_str())
                    .unwrap_or("(no summary)")
            );
            Ok(())
        }
        Some("fail") => {
            let summary = response
                .get("summary")
                .and_then(|s| s.as_str())
                .unwrap_or("Agent reported failure");
            Err(format!("Agent could not resolve conflicts: {}", summary))
        }
        _ => Err("Agent response missing or invalid 'verdict' field".to_string()),
    }
}

/// Remove a git worktree, handling both the worktree and its directory.
fn remove_worktree(repo_path: &Path, worktree_path: &Path) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["worktree", "remove", "--force"])
        .arg(worktree_path)
        .output()
        .map_err(|e| AirlockError::Git(format!("Failed to remove worktree: {}", e)))?;

    if !output.status.success() {
        // Try to clean up the directory manually if git worktree remove fails
        let _ = std::fs::remove_dir_all(worktree_path);
        // Also prune stale worktrees
        let _ = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["worktree", "prune"])
            .output();
    }

    Ok(())
}
