//! Git worktree management for pipeline execution.
//!
//! Provides utilities for creating and managing git worktrees used during
//! stage-based pipeline processing.
//!
//! - [`create_run_worktree`] - Create worktree at head commit
//! - [`remove_run_worktree`] - Remove a run's worktree
//! - [`list_worktrees`] - List all worktrees for a repo
//!
//! Worktree path format: `~/.airlock/worktrees/<repo_id>/<run_id>`

use crate::error::{AirlockError, Result};
use crate::types::SplitHunk;
use std::path::Path;
use std::process::Command;

// =============================================================================
// Stage-Based Pipeline Functions (Recommended)
// =============================================================================

/// Creates a run worktree for stage-based pipeline execution.
///
/// This function creates a detached worktree at `worktree_path` checked out to `head_sha`.
/// The worktree is created at the exact commit specified, with no patches applied.
///
/// Worktree path format: `~/.airlock/worktrees/<repo_id>/<run_id>`
///
/// # Arguments
/// * `gate_path` - Path to the bare git repository (gate)
/// * `worktree_path` - Path where the worktree should be created
/// * `head_sha` - The commit SHA to checkout
///
/// # Returns
/// `Ok(())` on success, or an error if worktree creation fails.
///
/// # Example
/// ```ignore
/// use airlock_core::worktree::create_run_worktree;
/// use airlock_core::AirlockPaths;
///
/// let paths = AirlockPaths::new()?;
/// let worktree_path = paths.run_worktree("repo-123", "run-456");
/// create_run_worktree(&gate_path, &worktree_path, "abc123def")?;
/// ```
pub fn create_run_worktree(gate_path: &Path, worktree_path: &Path, head_sha: &str) -> Result<()> {
    // Ensure the parent directory exists
    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Create the worktree in detached HEAD mode
    let output = Command::new("git")
        .args([
            "worktree",
            "add",
            "--detach",
            worktree_path
                .to_str()
                .ok_or_else(|| AirlockError::Git("Invalid worktree path".into()))?,
            head_sha,
        ])
        .current_dir(gate_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AirlockError::Git(format!(
            "Failed to create run worktree: {}",
            stderr
        )));
    }

    tracing::debug!(
        "Created run worktree at {} from {}",
        worktree_path.display(),
        head_sha
    );

    Ok(())
}

/// Removes a run worktree and cleans up associated git metadata.
///
/// This is a convenience wrapper around [`remove_worktree`] for the stage-based pipeline.
///
/// # Arguments
/// * `gate_path` - Path to the bare git repository (gate)
/// * `worktree_path` - Path to the worktree to remove
///
/// # Returns
/// `Ok(())` on success, or an error if removal fails.
///
/// # Example
/// ```ignore
/// use airlock_core::worktree::remove_run_worktree;
/// use airlock_core::AirlockPaths;
///
/// let paths = AirlockPaths::new()?;
/// let worktree_path = paths.run_worktree("repo-123", "run-456");
/// remove_run_worktree(&gate_path, &worktree_path)?;
/// ```
pub fn remove_run_worktree(gate_path: &Path, worktree_path: &Path) -> Result<()> {
    remove_worktree(gate_path, worktree_path)
}

/// Checks whether a worktree directory is valid by verifying its `.git` file
/// points to an existing git metadata directory.
///
/// A worktree contains a `.git` *file* (not directory) with content like:
/// `gitdir: /path/to/bare.git/worktrees/<name>`
///
/// If that target directory doesn't exist (e.g., the gate repo was deleted
/// and recreated), the worktree is stale and must be recreated.
pub fn is_valid_worktree(worktree_path: &Path) -> bool {
    let dot_git = worktree_path.join(".git");
    if dot_git.is_dir() {
        // A regular .git directory (not a worktree link) — treat as valid
        return true;
    }
    match std::fs::read_to_string(&dot_git) {
        Ok(contents) => {
            // Expected format: "gitdir: /absolute/path\n"
            if let Some(gitdir) = contents.trim().strip_prefix("gitdir:") {
                let gitdir_path = Path::new(gitdir.trim());
                gitdir_path.exists()
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

/// Resets a persistent worktree to the given commit, preserving gitignored files.
///
/// If the worktree doesn't exist yet, creates it via `git worktree add --detach`.
/// If it already exists, resets tracked files via `git reset --hard` and removes
/// untracked (but not gitignored) files via `git clean -fd`.
///
/// The key property: gitignored directories like `target/`, `node_modules/`,
/// `.build/` etc. are preserved across runs, so build caches survive.
///
/// # Arguments
/// * `gate_path` - Path to the bare git repository (gate)
/// * `worktree_path` - Path where the persistent worktree lives
/// * `head_sha` - The commit SHA to reset to
pub fn reset_persistent_worktree(
    gate_path: &Path,
    worktree_path: &Path,
    head_sha: &str,
) -> Result<()> {
    if worktree_path.exists() && !is_valid_worktree(worktree_path) {
        // Worktree directory exists but is broken (e.g., the gate repo was
        // deleted and recreated, destroying the internal git metadata that
        // the .git file points to). Remove the stale directory so we can
        // recreate it cleanly.
        tracing::warn!(
            "Persistent worktree at {} is stale (git metadata missing), removing",
            worktree_path.display(),
        );
        std::fs::remove_dir_all(worktree_path)?;
        // Prune any stale worktree entries in the gate repo
        let _ = Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(gate_path)
            .output();
    }

    if !worktree_path.exists() {
        // First time (or after stale cleanup): create the worktree
        tracing::info!(
            "Creating persistent worktree at {} for {}",
            worktree_path.display(),
            head_sha
        );
        return create_run_worktree(gate_path, worktree_path, head_sha);
    }

    // Worktree exists: fetch the new commit and reset
    tracing::info!(
        "Resetting persistent worktree at {} to {}",
        worktree_path.display(),
        head_sha
    );

    // Reset all tracked files to match the target commit.
    // git reset --hard moves HEAD and resets the working tree in one step,
    // even when dirty tracked files exist (unlike git checkout --detach).
    let output = Command::new("git")
        .args(["reset", "--hard", head_sha])
        .current_dir(worktree_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        // If reset fails because the worktree is stale (e.g., gate was
        // deleted and recreated), recover by removing and recreating it.
        // This catches cases where is_valid_worktree didn't detect staleness.
        if stderr.contains("not a git repository") {
            tracing::warn!(
                "Persistent worktree at {} is stale (reset failed: {}), recovering",
                worktree_path.display(),
                stderr.trim(),
            );
            std::fs::remove_dir_all(worktree_path)?;
            let _ = Command::new("git")
                .args(["worktree", "prune"])
                .current_dir(gate_path)
                .output();
            return create_run_worktree(gate_path, worktree_path, head_sha);
        }

        return Err(AirlockError::Git(format!(
            "Failed to reset persistent worktree: {}",
            stderr
        )));
    }

    // Remove untracked files/dirs but PRESERVE gitignored ones (no -x flag).
    // This is the key to keeping build caches alive.
    let output = Command::new("git")
        .args(["clean", "-fd"])
        .current_dir(worktree_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AirlockError::Git(format!(
            "Failed to clean persistent worktree: {}",
            stderr
        )));
    }

    tracing::debug!(
        "Reset persistent worktree at {} to {}",
        worktree_path.display(),
        head_sha
    );

    Ok(())
}

// =============================================================================
// Legacy Intent-Centric Pipeline Functions (DEPRECATED)
// =============================================================================

/// Creates a new git worktree at the specified path, checked out at the given base SHA.
///
/// **DEPRECATED**: This function is part of the legacy intent-centric pipeline
/// and will be removed in steps 10.13-10.16. Use [`create_run_worktree`] instead
/// for the new stage-based pipeline.
///
/// This function:
/// 1. Creates a detached worktree at `worktree_path` from `base_sha`
/// 2. Applies the provided hunks as a patch to the worktree
///
/// # Arguments
/// * `gate_path` - Path to the bare git repository (gate)
/// * `worktree_path` - Path where the worktree should be created
/// * `base_sha` - The commit SHA to use as the base
/// * `hunks` - The hunks to apply to the worktree
///
/// # Returns
/// `Ok(())` on success, or an error if worktree creation or patch application fails.
#[deprecated(
    since = "0.1.0",
    note = "Part of legacy intent-centric pipeline. Use create_run_worktree for stage-based pipeline."
)]
#[allow(deprecated)]
pub fn create_intent_worktree(
    gate_path: &Path,
    worktree_path: &Path,
    base_sha: &str,
    hunks: &[SplitHunk],
) -> Result<()> {
    // Ensure the parent directory exists
    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Create the worktree in detached HEAD mode
    let output = Command::new("git")
        .args([
            "worktree",
            "add",
            "--detach",
            worktree_path
                .to_str()
                .ok_or_else(|| AirlockError::Git("Invalid worktree path".into()))?,
            base_sha,
        ])
        .current_dir(gate_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AirlockError::Git(format!(
            "Failed to create worktree: {}",
            stderr
        )));
    }

    tracing::debug!(
        "Created worktree at {} from {}",
        worktree_path.display(),
        base_sha
    );

    // Generate and apply the patch if there are hunks
    if !hunks.is_empty() {
        let patch = hunks_to_patch(hunks);
        apply_patch(worktree_path, &patch)?;
    }

    Ok(())
}

/// Generates a unified diff patch from a list of hunks.
///
/// **DEPRECATED**: This function is part of the legacy intent-centric pipeline
/// and will be removed in steps 10.13-10.16. The stage-based pipeline uses
/// worktrees at exact commits without patches.
///
/// The generated patch follows the unified diff format and can be applied
/// using `git apply`.
///
/// # Arguments
/// * `hunks` - The hunks to include in the patch
///
/// # Returns
/// A string containing the unified diff patch.
#[deprecated(
    since = "0.1.0",
    note = "Part of legacy intent-centric pipeline. Stage-based pipeline doesn't use patches."
)]
pub fn hunks_to_patch(hunks: &[SplitHunk]) -> String {
    if hunks.is_empty() {
        return String::new();
    }

    let mut patch = String::new();

    // Group hunks by file path to generate proper diff headers
    let mut hunks_by_file: std::collections::HashMap<&str, Vec<&SplitHunk>> =
        std::collections::HashMap::new();

    for hunk in hunks {
        hunks_by_file.entry(&hunk.file_path).or_default().push(hunk);
    }

    // Sort files for deterministic output
    let mut file_paths: Vec<&&str> = hunks_by_file.keys().collect();
    file_paths.sort();

    for file_path in file_paths {
        let file_hunks = hunks_by_file.get(file_path).unwrap();

        // Sort hunks by index for proper ordering
        let mut sorted_hunks: Vec<&&SplitHunk> = file_hunks.iter().collect();
        sorted_hunks.sort_by_key(|h| h.hunk_index);

        // Add file header
        patch.push_str(&format!("diff --git a/{} b/{}\n", file_path, file_path));
        patch.push_str(&format!("--- a/{}\n", file_path));
        patch.push_str(&format!("+++ b/{}\n", file_path));

        // Add each hunk
        for hunk in sorted_hunks {
            // The hunk content should already include the @@ header
            // If not, we need to add it
            let content = &hunk.content;
            if content.starts_with("@@") {
                patch.push_str(content);
            } else {
                // Add the @@ header if missing
                patch.push_str(&format!(
                    "@@ -{},{} +{},{} @@\n",
                    hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
                ));
                patch.push_str(content);
            }

            // Ensure patch ends with newline
            if !patch.ends_with('\n') {
                patch.push('\n');
            }
        }
    }

    patch
}

/// Applies a unified diff patch to a worktree.
///
/// **DEPRECATED**: This function is part of the legacy intent-centric pipeline
/// and will be removed in steps 10.13-10.16. The stage-based pipeline uses
/// worktrees at exact commits without patches.
///
/// # Arguments
/// * `worktree_path` - Path to the worktree where the patch should be applied
/// * `patch` - The unified diff patch content
///
/// # Returns
/// `Ok(())` on success, or an error if patch application fails.
#[deprecated(
    since = "0.1.0",
    note = "Part of legacy intent-centric pipeline. Stage-based pipeline doesn't use patches."
)]
pub fn apply_patch(worktree_path: &Path, patch: &str) -> Result<()> {
    if patch.is_empty() {
        return Ok(());
    }

    // Use git apply with the patch content via stdin
    let mut child = Command::new("git")
        .args(["apply", "--verbose", "-"])
        .current_dir(worktree_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // Write patch to stdin
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin.write_all(patch.as_bytes())?;
    }

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AirlockError::Git(format!(
            "Failed to apply patch: {}",
            stderr
        )));
    }

    tracing::debug!("Applied patch to worktree at {}", worktree_path.display());
    Ok(())
}

/// Creates a branch in the worktree and commits all changes.
///
/// **DEPRECATED**: This function is part of the legacy intent-centric pipeline
/// and will be removed in steps 10.13-10.16. The stage-based pipeline manages
/// branches through the `airlock exec push` and `airlock exec create-pr` stages.
///
/// # Arguments
/// * `worktree_path` - Path to the worktree
/// * `branch_name` - Name of the branch to create
/// * `commit_msg` - Commit message
///
/// # Returns
/// The commit SHA on success.
#[deprecated(
    since = "0.1.0",
    note = "Part of legacy intent-centric pipeline. Stage-based pipeline uses airlock exec commands."
)]
pub fn create_intent_branch(
    worktree_path: &Path,
    branch_name: &str,
    commit_msg: &str,
) -> Result<String> {
    // Create the branch
    let output = Command::new("git")
        .args(["checkout", "-b", branch_name])
        .current_dir(worktree_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AirlockError::Git(format!(
            "Failed to create branch '{}': {}",
            branch_name, stderr
        )));
    }

    // Stage all changes
    let output = Command::new("git")
        .args(["add", "-A"])
        .current_dir(worktree_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AirlockError::Git(format!(
            "Failed to stage changes: {}",
            stderr
        )));
    }

    // Create the commit
    let output = Command::new("git")
        .args(["commit", "-m", commit_msg, "--allow-empty"])
        .current_dir(worktree_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AirlockError::Git(format!("Failed to commit: {stderr}")));
    }

    // Get the commit SHA
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(worktree_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AirlockError::Git(format!(
            "Failed to get commit SHA: {}",
            stderr
        )));
    }

    let commit_sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    tracing::debug!(
        "Created branch '{}' with commit {}",
        branch_name,
        commit_sha
    );

    Ok(commit_sha)
}

/// Removes a worktree and its associated files.
///
/// # Arguments
/// * `gate_path` - Path to the bare git repository (gate)
/// * `worktree_path` - Path to the worktree to remove
///
/// # Returns
/// `Ok(())` on success, or an error if removal fails.
pub fn remove_worktree(gate_path: &Path, worktree_path: &Path) -> Result<()> {
    // First try to remove the worktree gracefully
    let output = Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            worktree_path
                .to_str()
                .ok_or_else(|| AirlockError::Git("Invalid worktree path".into()))?,
        ])
        .current_dir(gate_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("git worktree remove failed: {}", stderr);

        // Try to clean up manually if git command fails
        if worktree_path.exists() {
            std::fs::remove_dir_all(worktree_path)?;
            tracing::debug!(
                "Manually removed worktree directory at {}",
                worktree_path.display()
            );
        }

        // Prune stale worktree entries
        let _ = Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(gate_path)
            .output();
    } else {
        tracing::debug!("Removed worktree at {}", worktree_path.display());
    }

    Ok(())
}

/// Lists all worktrees for a repository.
///
/// # Arguments
/// * `gate_path` - Path to the bare git repository (gate)
///
/// # Returns
/// A vector of worktree paths.
pub fn list_worktrees(gate_path: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(gate_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AirlockError::Git(format!(
            "Failed to list worktrees: {}",
            stderr
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut worktrees = Vec::new();

    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            worktrees.push(path.to_string());
        }
    }

    Ok(worktrees)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shared helper to create a bare repo with an initial commit containing a file.
    ///
    /// Used by both `integration_legacy` and `integration_stage_based` test modules.
    #[cfg(unix)]
    fn create_bare_repo_with_file(
        temp_dir: &tempfile::TempDir,
        file_name: &str,
        file_content: &str,
    ) -> (std::path::PathBuf, String) {
        use std::process::Command;

        // Create a working repo first
        let work_path = temp_dir.path().join("work");
        std::fs::create_dir_all(&work_path).unwrap();

        // Init the repo
        Command::new("git")
            .args(["init"])
            .current_dir(&work_path)
            .output()
            .unwrap();

        // Configure git user
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&work_path)
            .output()
            .unwrap();

        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(&work_path)
            .output()
            .unwrap();

        // Create the file
        std::fs::write(work_path.join(file_name), file_content).unwrap();

        // Add and commit
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(&work_path)
            .output()
            .unwrap();

        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(&work_path)
            .output()
            .unwrap();

        // Get the commit SHA
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&work_path)
            .output()
            .unwrap();
        let commit_sha = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // Clone to bare repo
        let bare_path = temp_dir.path().join("bare.git");
        Command::new("git")
            .args([
                "clone",
                "--bare",
                work_path.to_str().unwrap(),
                bare_path.to_str().unwrap(),
            ])
            .output()
            .unwrap();

        (bare_path, commit_sha)
    }

    // =========================================================================
    // Legacy Unit Tests (for deprecated functions)
    // These tests use deprecated functions and will be removed in 10.13-10.16
    // =========================================================================

    #[test]
    #[allow(deprecated)]
    fn test_hunks_to_patch_empty() {
        let patch = hunks_to_patch(&[]);
        assert!(patch.is_empty());
    }

    #[test]
    #[allow(deprecated)]
    fn test_hunks_to_patch_single_hunk() {
        let hunk = SplitHunk {
            id: "src/main.rs:0".to_string(),
            file_path: "src/main.rs".to_string(),
            hunk_index: 0,
            old_start: 1,
            old_lines: 3,
            new_start: 1,
            new_lines: 4,
            additions: 1,
            deletions: 0,
            content: "@@ -1,3 +1,4 @@\n fn main() {\n     println!(\"Hello\");\n+    println!(\"World\");\n }\n".to_string(),
            language: Some("rust".to_string()),
        };

        let patch = hunks_to_patch(&[hunk]);

        assert!(patch.contains("diff --git a/src/main.rs b/src/main.rs"));
        assert!(patch.contains("--- a/src/main.rs"));
        assert!(patch.contains("+++ b/src/main.rs"));
        assert!(patch.contains("@@ -1,3 +1,4 @@"));
        assert!(patch.contains("+    println!(\"World\");"));
    }

    #[test]
    #[allow(deprecated)]
    fn test_hunks_to_patch_multiple_files() {
        let hunk1 = SplitHunk {
            id: "src/lib.rs:0".to_string(),
            file_path: "src/lib.rs".to_string(),
            hunk_index: 0,
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 2,
            additions: 1,
            deletions: 0,
            content: "@@ -1,1 +1,2 @@\n pub mod foo;\n+pub mod bar;\n".to_string(),
            language: Some("rust".to_string()),
        };

        let hunk2 = SplitHunk {
            id: "src/main.rs:0".to_string(),
            file_path: "src/main.rs".to_string(),
            hunk_index: 0,
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 2,
            additions: 1,
            deletions: 0,
            content: "@@ -1,1 +1,2 @@\n fn main() {}\n+// comment\n".to_string(),
            language: Some("rust".to_string()),
        };

        let patch = hunks_to_patch(&[hunk1, hunk2]);

        // Both files should be in the patch
        assert!(patch.contains("diff --git a/src/lib.rs b/src/lib.rs"));
        assert!(patch.contains("diff --git a/src/main.rs b/src/main.rs"));
    }

    #[test]
    #[allow(deprecated)]
    fn test_hunks_to_patch_multiple_hunks_same_file() {
        let hunk1 = SplitHunk {
            id: "src/main.rs:0".to_string(),
            file_path: "src/main.rs".to_string(),
            hunk_index: 0,
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 2,
            additions: 1,
            deletions: 0,
            content: "@@ -1,1 +1,2 @@\n fn main() {\n+    // first\n".to_string(),
            language: Some("rust".to_string()),
        };

        let hunk2 = SplitHunk {
            id: "src/main.rs:1".to_string(),
            file_path: "src/main.rs".to_string(),
            hunk_index: 1,
            old_start: 10,
            old_lines: 1,
            new_start: 11,
            new_lines: 2,
            additions: 1,
            deletions: 0,
            content: "@@ -10,1 +11,2 @@\n fn other() {\n+    // second\n".to_string(),
            language: Some("rust".to_string()),
        };

        let patch = hunks_to_patch(&[hunk2, hunk1]); // Note: reversed order

        // Should only have one file header
        let diff_count = patch
            .matches("diff --git a/src/main.rs b/src/main.rs")
            .count();
        assert_eq!(diff_count, 1, "Should have exactly one file header");

        // Both hunks should be present and in order by hunk_index
        let first_pos = patch.find("// first").unwrap();
        let second_pos = patch.find("// second").unwrap();
        assert!(first_pos < second_pos, "Hunks should be ordered by index");
    }

    #[test]
    #[allow(deprecated)]
    fn test_hunks_to_patch_adds_header_if_missing() {
        let hunk = SplitHunk {
            id: "src/main.rs:0".to_string(),
            file_path: "src/main.rs".to_string(),
            hunk_index: 0,
            old_start: 1,
            old_lines: 3,
            new_start: 1,
            new_lines: 4,
            additions: 1,
            deletions: 0,
            content: " fn main() {\n     println!(\"Hello\");\n+    println!(\"World\");\n }\n"
                .to_string(),
            language: Some("rust".to_string()),
        };

        let patch = hunks_to_patch(&[hunk]);

        // Should have added the @@ header
        assert!(patch.contains("@@ -1,3 +1,4 @@"));
    }

    // =========================================================================
    // Legacy Integration Tests (DEPRECATED - for intent-centric pipeline)
    // These tests use deprecated functions and will be removed in 10.13-10.16
    // =========================================================================

    #[cfg(unix)]
    #[allow(deprecated)]
    mod integration_legacy {
        use super::*;
        use std::fs;
        use std::process::Command;
        use tempfile::TempDir;

        #[test]
        fn test_create_and_remove_worktree() {
            let temp_dir = TempDir::new().unwrap();
            let (bare_path, commit_sha) =
                create_bare_repo_with_file(&temp_dir, "README.md", "# Hello World\n");

            let worktree_path = temp_dir.path().join("worktree");

            // Create worktree without hunks (no changes to apply)
            create_intent_worktree(&bare_path, &worktree_path, &commit_sha, &[]).unwrap();

            // Verify worktree was created
            assert!(worktree_path.exists());
            assert!(worktree_path.join("README.md").exists());

            // Verify the README content
            let content = fs::read_to_string(worktree_path.join("README.md")).unwrap();
            assert_eq!(content, "# Hello World\n");

            // List worktrees and verify our worktree is there
            let worktrees = list_worktrees(&bare_path).unwrap();
            assert!(worktrees.len() >= 1); // At least the main bare repo and our worktree

            // Remove the worktree
            remove_worktree(&bare_path, &worktree_path).unwrap();

            // Verify worktree was removed
            assert!(!worktree_path.exists());
        }

        #[test]
        fn test_apply_patch_to_worktree() {
            let temp_dir = TempDir::new().unwrap();
            let (bare_path, commit_sha) =
                create_bare_repo_with_file(&temp_dir, "hello.txt", "Hello\nWorld\n");

            let worktree_path = temp_dir.path().join("worktree");

            // Create worktree first (without hunks)
            create_intent_worktree(&bare_path, &worktree_path, &commit_sha, &[]).unwrap();

            // Apply a patch directly
            let patch = r#"diff --git a/hello.txt b/hello.txt
--- a/hello.txt
+++ b/hello.txt
@@ -1,2 +1,3 @@
 Hello
+Beautiful
 World
"#;

            apply_patch(&worktree_path, patch).unwrap();

            // Verify the file was modified
            let content = fs::read_to_string(worktree_path.join("hello.txt")).unwrap();
            assert_eq!(content, "Hello\nBeautiful\nWorld\n");

            // Cleanup
            remove_worktree(&bare_path, &worktree_path).unwrap();
        }

        #[test]
        fn test_create_intent_branch() {
            let temp_dir = TempDir::new().unwrap();
            let (bare_path, commit_sha) =
                create_bare_repo_with_file(&temp_dir, "test.txt", "initial content\n");

            let worktree_path = temp_dir.path().join("worktree");

            // Create worktree
            create_intent_worktree(&bare_path, &worktree_path, &commit_sha, &[]).unwrap();

            // Configure git user in worktree
            Command::new("git")
                .args(["config", "user.email", "test@example.com"])
                .current_dir(&worktree_path)
                .output()
                .unwrap();

            Command::new("git")
                .args(["config", "user.name", "Test User"])
                .current_dir(&worktree_path)
                .output()
                .unwrap();

            // Modify a file
            fs::write(worktree_path.join("test.txt"), "modified content\n").unwrap();

            // Create branch and commit
            let new_sha = create_intent_branch(
                &worktree_path,
                "airlock/run-123/intent-456",
                "Test commit message",
            )
            .unwrap();

            // Verify the branch was created and commit SHA is valid
            assert!(!new_sha.is_empty());
            assert!(new_sha.len() >= 7); // Short SHA at minimum

            // Verify we're on the correct branch
            let output = Command::new("git")
                .args(["branch", "--show-current"])
                .current_dir(&worktree_path)
                .output()
                .unwrap();
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            assert_eq!(branch, "airlock/run-123/intent-456");

            // Cleanup
            remove_worktree(&bare_path, &worktree_path).unwrap();
        }

        #[test]
        fn test_create_worktree_with_hunks() {
            let temp_dir = TempDir::new().unwrap();
            let (bare_path, commit_sha) =
                create_bare_repo_with_file(&temp_dir, "file.txt", "line1\nline2\nline3\n");

            let worktree_path = temp_dir.path().join("worktree");

            // Create a hunk that adds a line
            let hunk = SplitHunk {
                id: "file.txt:0".to_string(),
                file_path: "file.txt".to_string(),
                hunk_index: 0,
                old_start: 1,
                old_lines: 3,
                new_start: 1,
                new_lines: 4,
                additions: 1,
                deletions: 0,
                content: "@@ -1,3 +1,4 @@\n line1\n+inserted\n line2\n line3\n".to_string(),
                language: None,
            };

            // Create worktree with hunks applied
            create_intent_worktree(&bare_path, &worktree_path, &commit_sha, &[hunk]).unwrap();

            // Verify the file has the inserted line
            let content = fs::read_to_string(worktree_path.join("file.txt")).unwrap();
            assert_eq!(content, "line1\ninserted\nline2\nline3\n");

            // Cleanup
            remove_worktree(&bare_path, &worktree_path).unwrap();
        }

        #[test]
        fn test_full_intent_workflow() {
            // This test simulates the full workflow:
            // 1. Create bare repo with initial file
            // 2. Create worktree with hunks
            // 3. Create branch and commit
            // 4. Verify the branch in the bare repo
            // 5. Remove worktree

            let temp_dir = TempDir::new().unwrap();
            let (bare_path, commit_sha) =
                create_bare_repo_with_file(&temp_dir, "code.rs", "fn main() {\n}\n");

            let worktree_path = temp_dir.path().join("intent-worktree");

            // Create hunk to add code
            let hunk = SplitHunk {
                id: "code.rs:0".to_string(),
                file_path: "code.rs".to_string(),
                hunk_index: 0,
                old_start: 1,
                old_lines: 2,
                new_start: 1,
                new_lines: 3,
                additions: 1,
                deletions: 0,
                content: "@@ -1,2 +1,3 @@\n fn main() {\n+    println!(\"Hello\");\n }\n"
                    .to_string(),
                language: Some("rust".to_string()),
            };

            // Create worktree with hunks
            create_intent_worktree(&bare_path, &worktree_path, &commit_sha, &[hunk]).unwrap();

            // Configure git user
            Command::new("git")
                .args(["config", "user.email", "test@example.com"])
                .current_dir(&worktree_path)
                .output()
                .unwrap();
            Command::new("git")
                .args(["config", "user.name", "Test User"])
                .current_dir(&worktree_path)
                .output()
                .unwrap();

            // Create branch and commit
            let _commit_sha = create_intent_branch(
                &worktree_path,
                "airlock/run-abc/intent-1",
                "feat: Add hello world print",
            )
            .unwrap();

            // Verify the branch exists in the bare repo
            let output = Command::new("git")
                .args(["branch", "-a"])
                .current_dir(&bare_path)
                .output()
                .unwrap();
            let branches = String::from_utf8_lossy(&output.stdout);
            // The branch should be visible somehow
            assert!(
                branches.contains("airlock/run-abc/intent-1"),
                "Branch should be created. Branches: {}",
                branches
            );

            // Verify the file content in the worktree
            let content = fs::read_to_string(worktree_path.join("code.rs")).unwrap();
            assert!(content.contains("println!(\"Hello\");"));

            // Cleanup
            remove_worktree(&bare_path, &worktree_path).unwrap();
            assert!(!worktree_path.exists());
        }
    }

    // =========================================================================
    // Stage-Based Pipeline Integration Tests (Recommended)
    // =========================================================================

    #[cfg(unix)]
    mod integration_stage_based {
        use super::*;
        use std::fs;
        use std::process::Command;
        use tempfile::TempDir;

        #[test]
        fn test_create_run_worktree() {
            let temp_dir = TempDir::new().unwrap();
            let (bare_path, commit_sha) =
                create_bare_repo_with_file(&temp_dir, "README.md", "# Test\n");

            // Create worktree path with format: worktrees/<repo>/<run>
            let worktree_path = temp_dir
                .path()
                .join("worktrees")
                .join("repo-123")
                .join("run-456");

            // Create run worktree
            create_run_worktree(&bare_path, &worktree_path, &commit_sha).unwrap();

            // Verify worktree was created
            assert!(worktree_path.exists());
            assert!(worktree_path.join("README.md").exists());

            // Verify the README content
            let content = fs::read_to_string(worktree_path.join("README.md")).unwrap();
            assert_eq!(content, "# Test\n");

            // List worktrees and verify our worktree is there
            let worktrees = list_worktrees(&bare_path).unwrap();
            assert!(worktrees.len() >= 1);

            // Cleanup
            remove_worktree(&bare_path, &worktree_path).unwrap();
            assert!(!worktree_path.exists());
        }

        #[test]
        fn test_remove_run_worktree() {
            let temp_dir = TempDir::new().unwrap();
            let (bare_path, commit_sha) =
                create_bare_repo_with_file(&temp_dir, "file.txt", "content\n");

            let worktree_path = temp_dir
                .path()
                .join("worktrees")
                .join("repo-abc")
                .join("run-xyz");

            // Create and then remove worktree using the convenience function
            create_run_worktree(&bare_path, &worktree_path, &commit_sha).unwrap();
            assert!(worktree_path.exists());

            remove_run_worktree(&bare_path, &worktree_path).unwrap();
            assert!(!worktree_path.exists());
        }

        #[test]
        fn test_run_worktree_at_specific_commit() {
            let temp_dir = TempDir::new().unwrap();

            // Create a working repo with multiple commits
            let work_path = temp_dir.path().join("work");
            fs::create_dir_all(&work_path).unwrap();

            Command::new("git")
                .args(["init"])
                .current_dir(&work_path)
                .output()
                .unwrap();

            Command::new("git")
                .args(["config", "user.email", "test@example.com"])
                .current_dir(&work_path)
                .output()
                .unwrap();

            Command::new("git")
                .args(["config", "user.name", "Test User"])
                .current_dir(&work_path)
                .output()
                .unwrap();

            // First commit
            fs::write(work_path.join("file.txt"), "version 1\n").unwrap();
            Command::new("git")
                .args(["add", "-A"])
                .current_dir(&work_path)
                .output()
                .unwrap();
            Command::new("git")
                .args(["commit", "-m", "First commit"])
                .current_dir(&work_path)
                .output()
                .unwrap();

            // Get first commit SHA
            let output = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&work_path)
                .output()
                .unwrap();
            let first_sha = String::from_utf8_lossy(&output.stdout).trim().to_string();

            // Second commit
            fs::write(work_path.join("file.txt"), "version 2\n").unwrap();
            Command::new("git")
                .args(["add", "-A"])
                .current_dir(&work_path)
                .output()
                .unwrap();
            Command::new("git")
                .args(["commit", "-m", "Second commit"])
                .current_dir(&work_path)
                .output()
                .unwrap();

            let output = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&work_path)
                .output()
                .unwrap();
            let second_sha = String::from_utf8_lossy(&output.stdout).trim().to_string();

            // Clone to bare repo
            let bare_path = temp_dir.path().join("bare.git");
            Command::new("git")
                .args([
                    "clone",
                    "--bare",
                    work_path.to_str().unwrap(),
                    bare_path.to_str().unwrap(),
                ])
                .output()
                .unwrap();

            // Create worktree at first commit
            let worktree1 = temp_dir.path().join("worktrees").join("repo").join("run1");
            create_run_worktree(&bare_path, &worktree1, &first_sha).unwrap();
            let content1 = fs::read_to_string(worktree1.join("file.txt")).unwrap();
            assert_eq!(content1, "version 1\n");

            // Create worktree at second commit
            let worktree2 = temp_dir.path().join("worktrees").join("repo").join("run2");
            create_run_worktree(&bare_path, &worktree2, &second_sha).unwrap();
            let content2 = fs::read_to_string(worktree2.join("file.txt")).unwrap();
            assert_eq!(content2, "version 2\n");

            // Cleanup
            remove_run_worktree(&bare_path, &worktree1).unwrap();
            remove_run_worktree(&bare_path, &worktree2).unwrap();
        }

        #[test]
        fn test_multiple_run_worktrees() {
            let temp_dir = TempDir::new().unwrap();
            let (bare_path, commit_sha) =
                create_bare_repo_with_file(&temp_dir, "README.md", "# Test\n");

            // Create multiple worktrees for different runs
            let worktree1 = temp_dir
                .path()
                .join("worktrees")
                .join("repo-1")
                .join("run-a");
            let worktree2 = temp_dir
                .path()
                .join("worktrees")
                .join("repo-1")
                .join("run-b");
            let worktree3 = temp_dir
                .path()
                .join("worktrees")
                .join("repo-2")
                .join("run-c");

            create_run_worktree(&bare_path, &worktree1, &commit_sha).unwrap();
            create_run_worktree(&bare_path, &worktree2, &commit_sha).unwrap();
            create_run_worktree(&bare_path, &worktree3, &commit_sha).unwrap();

            // All worktrees should exist
            assert!(worktree1.exists());
            assert!(worktree2.exists());
            assert!(worktree3.exists());

            // List worktrees
            let worktrees = list_worktrees(&bare_path).unwrap();
            // Should have at least 3 worktrees (plus possibly the bare repo itself)
            assert!(worktrees.len() >= 3);

            // Cleanup
            remove_run_worktree(&bare_path, &worktree1).unwrap();
            remove_run_worktree(&bare_path, &worktree2).unwrap();
            remove_run_worktree(&bare_path, &worktree3).unwrap();
        }

        #[test]
        fn test_worktree_path_format() {
            // Verify the path format matches the spec: ~/.airlock/worktrees/<repo>/<run>
            use crate::paths::AirlockPaths;

            let paths = AirlockPaths::with_root(std::path::PathBuf::from("/tmp/airlock"));
            let worktree_path = paths.run_worktree("repo-123", "run-456");

            assert_eq!(
                worktree_path.to_str().unwrap(),
                "/tmp/airlock/worktrees/repo-123/run-456"
            );
        }

        #[test]
        fn test_reset_persistent_worktree_recovers_from_stale_gitdir() {
            // Simulates the scenario where the gate repo was deleted and
            // recreated (e.g., eject + init), leaving a stale worktree
            // whose .git file points to a non-existent metadata directory.
            let temp_dir = TempDir::new().unwrap();
            let (bare_path, commit_sha) =
                create_bare_repo_with_file(&temp_dir, "README.md", "# Test\n");

            let worktree_path = temp_dir
                .path()
                .join("worktrees")
                .join("repo-1")
                .join("persistent");

            // Create the persistent worktree normally
            reset_persistent_worktree(&bare_path, &worktree_path, &commit_sha).unwrap();
            assert!(worktree_path.exists());
            assert!(worktree_path.join("README.md").exists());

            // Simulate gate deletion: remove the internal worktree metadata
            // that the .git file points to (inside the bare repo).
            let internal_wt_dir = bare_path.join("worktrees").join("persistent");
            assert!(
                internal_wt_dir.exists(),
                "internal worktree metadata should exist"
            );
            fs::remove_dir_all(&internal_wt_dir).unwrap();
            assert!(!internal_wt_dir.exists());

            // The worktree dir still exists but is now stale
            assert!(worktree_path.exists());

            // reset_persistent_worktree should detect the stale state,
            // remove the directory, and recreate it cleanly.
            reset_persistent_worktree(&bare_path, &worktree_path, &commit_sha).unwrap();

            // Verify the worktree is functional again
            assert!(worktree_path.exists());
            assert!(worktree_path.join("README.md").exists());
            let content = fs::read_to_string(worktree_path.join("README.md")).unwrap();
            assert_eq!(content, "# Test\n");

            // Verify git commands work in the recovered worktree
            let output = Command::new("git")
                .args(["status"])
                .current_dir(&worktree_path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git status should work in recovered worktree"
            );

            // Cleanup
            remove_run_worktree(&bare_path, &worktree_path).unwrap();
        }

        #[test]
        fn test_reset_persistent_worktree_recovers_from_checkout_failure() {
            // Simulates the scenario where is_valid_worktree doesn't catch
            // staleness but git checkout fails with "not a git repository".
            // This can happen when the .git file exists and points to a path
            // that is partially valid (e.g., the parent dir exists but internal
            // metadata is corrupted).
            let temp_dir = TempDir::new().unwrap();
            let (bare_path, commit_sha) =
                create_bare_repo_with_file(&temp_dir, "README.md", "# Test\n");

            let worktree_path = temp_dir
                .path()
                .join("worktrees")
                .join("repo-1")
                .join("persistent");

            // Create the persistent worktree normally
            reset_persistent_worktree(&bare_path, &worktree_path, &commit_sha).unwrap();
            assert!(worktree_path.exists());

            // Corrupt the worktree's .git file to point to a non-existent
            // directory that will bypass is_valid_worktree (we keep the
            // parent directory so the path "exists" at the parent level)
            // but will cause git checkout to fail with "not a git repository".
            let dot_git = worktree_path.join(".git");
            let bogus_gitdir = temp_dir.path().join("bogus_gitdir");
            fs::create_dir_all(&bogus_gitdir).unwrap();
            fs::write(&dot_git, format!("gitdir: {}\n", bogus_gitdir.display())).unwrap();

            // The worktree exists and .git points to a directory that exists,
            // so is_valid_worktree returns true — but git checkout will fail.
            assert!(worktree_path.exists());

            // reset_persistent_worktree should recover via the checkout
            // failure path: remove, prune, and recreate.
            reset_persistent_worktree(&bare_path, &worktree_path, &commit_sha).unwrap();

            // Verify the worktree is functional
            assert!(worktree_path.exists());
            assert!(worktree_path.join("README.md").exists());
            let content = fs::read_to_string(worktree_path.join("README.md")).unwrap();
            assert_eq!(content, "# Test\n");

            // Verify git commands work
            let output = Command::new("git")
                .args(["status"])
                .current_dir(&worktree_path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git status should work in recovered worktree"
            );

            // Cleanup
            remove_run_worktree(&bare_path, &worktree_path).unwrap();
        }
    }
}
