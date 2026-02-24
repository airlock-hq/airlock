//! Diff utilities for Airlock.
//!
//! Provides functions for computing diffs between commits, with special handling
//! for null SHAs (new branch creation) where there's no previous commit to diff against.

use super::refs::is_null_sha;
use std::path::Path;
use std::process::Command;
use tracing::debug;

/// The git empty tree SHA - used as a last resort fallback for diffs.
/// This is a well-known constant in git representing an empty tree.
pub const EMPTY_TREE_SHA: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Default branches to try when looking for a merge-base.
pub const DEFAULT_BRANCHES: &[&str] = &["origin/main", "origin/master", "main", "master"];

/// Result of computing a diff between two commits.
#[derive(Debug, Clone, Default)]
pub struct DiffResult {
    /// The unified diff patch content.
    pub patch: String,
    /// List of files changed.
    pub files_changed: Vec<String>,
    /// Number of lines added.
    pub additions: u32,
    /// Number of lines deleted.
    pub deletions: u32,
    /// The effective base SHA used for the diff (may differ from input if null SHA was provided).
    pub effective_base_sha: String,
}

/// Find the effective base SHA for computing a diff.
///
/// When `base_sha` is a null SHA (indicating a new branch creation), this function
/// finds an appropriate commit to diff against:
///
/// 1. First, try to find the merge-base with common default branches
///    (origin/main, origin/master, main, master)
/// 2. If no merge-base is found, try to find the root commit(s) of the repository
/// 3. As a last resort, use the git empty tree SHA (shows all files as added)
///
/// # Arguments
///
/// * `repo_path` - Path to the git repository (can be bare or working tree)
/// * `base_sha` - The original base SHA (may be null)
/// * `head_sha` - The head SHA to diff to
///
/// # Returns
///
/// The effective base SHA to use for diffing.
pub fn find_effective_base_sha(repo_path: &Path, base_sha: &str, head_sha: &str) -> String {
    if !is_null_sha(base_sha) {
        return base_sha.to_string();
    }

    debug!("Base SHA is null (new branch), finding suitable base...");

    // Try to find merge-base with default branches
    if let Some(merge_base) = find_merge_base(repo_path, head_sha, DEFAULT_BRANCHES) {
        debug!("Found merge-base: {}", merge_base);
        return merge_base;
    }

    // Try to find the root commit
    if let Some(root) = find_root_commit(repo_path, head_sha) {
        debug!("Using root commit as base: {}", root);
        return root;
    }

    // Last resort: use the empty tree SHA
    debug!("Using empty tree SHA as base");
    EMPTY_TREE_SHA.to_string()
}

/// Find the merge-base between a commit and one of the given branches.
///
/// Tries each branch in order and returns the first successful merge-base.
///
/// # Arguments
///
/// * `repo_path` - Path to the git repository
/// * `head_sha` - The commit to find the merge-base for
/// * `branches` - List of branch names to try (e.g., ["origin/main", "main"])
///
/// # Returns
///
/// The merge-base SHA if found, None otherwise.
pub fn find_merge_base(repo_path: &Path, head_sha: &str, branches: &[&str]) -> Option<String> {
    for branch in branches {
        let output = Command::new("git")
            .args([
                "-C",
                &repo_path.to_string_lossy(),
                "merge-base",
                branch,
                head_sha,
            ])
            .output();

        if let Ok(o) = output {
            if o.status.success() {
                let merge_base = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if !merge_base.is_empty() {
                    debug!("Found merge-base with {}: {}", branch, merge_base);
                    return Some(merge_base);
                }
            }
        }
    }

    None
}

/// Find the root commit(s) reachable from a given commit.
///
/// Uses `git rev-list --max-parents=0` to find commits with no parents.
///
/// # Arguments
///
/// * `repo_path` - Path to the git repository
/// * `head_sha` - The commit to start from
///
/// # Returns
///
/// The first root commit SHA if found, None otherwise.
pub fn find_root_commit(repo_path: &Path, head_sha: &str) -> Option<String> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "rev-list",
            "--max-parents=0",
            head_sha,
        ])
        .output();

    if let Ok(o) = output {
        if o.status.success() {
            let root = String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            if !root.is_empty() {
                return Some(root);
            }
        }
    }

    None
}

/// Compute a diff between two commits.
///
/// This function handles null SHAs (new branch creation) by automatically finding
/// an appropriate base commit to diff against.
///
/// # Arguments
///
/// * `repo_path` - Path to the git repository (can be bare or working tree)
/// * `base_sha` - The base SHA (may be null for new branches)
/// * `head_sha` - The head SHA
///
/// # Returns
///
/// A `DiffResult` containing the patch, files changed, and statistics.
pub fn compute_diff(repo_path: &Path, base_sha: &str, head_sha: &str) -> DiffResult {
    let effective_base = find_effective_base_sha(repo_path, base_sha, head_sha);

    let patch = get_diff_patch(repo_path, &effective_base, head_sha);
    let files_changed = get_files_changed(repo_path, &effective_base, head_sha);
    let (additions, deletions) = get_diff_stats(repo_path, &effective_base, head_sha);

    DiffResult {
        patch,
        files_changed,
        additions,
        deletions,
        effective_base_sha: effective_base,
    }
}

/// Information about a single commit.
#[derive(Debug, Clone)]
pub struct CommitInfo {
    /// Full commit SHA.
    pub sha: String,
    /// Commit message (first line / subject).
    pub message: String,
    /// Author name.
    pub author: String,
    /// Author timestamp (Unix epoch seconds).
    pub timestamp: i64,
}

/// Result of computing a diff with per-commit breakdown.
#[derive(Debug, Clone)]
pub struct CommitDiffResult {
    /// Overall diff between base and head.
    pub diff: DiffResult,
    /// Commits in the range (oldest first).
    pub commits: Vec<CommitInfo>,
    /// Per-commit unified diff patches (same order as `commits`).
    /// Empty for single-commit pushes.
    pub commit_patches: Vec<String>,
}

/// List commits between `base_sha` (exclusive) and `head_sha` (inclusive), oldest first.
pub fn list_commits(repo_path: &Path, base_sha: &str, head_sha: &str) -> Vec<CommitInfo> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "log",
            "--reverse",
            "--format=%H%n%s%n%an%n%at",
            &format!("{}..{}", base_sha, head_sha),
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let lines: Vec<&str> = stdout.lines().collect();
            let mut commits = Vec::new();
            // Each commit is 4 lines: sha, subject, author, timestamp
            for chunk in lines.chunks(4) {
                if chunk.len() == 4 {
                    commits.push(CommitInfo {
                        sha: chunk[0].to_string(),
                        message: chunk[1].to_string(),
                        author: chunk[2].to_string(),
                        timestamp: chunk[3].parse::<i64>().unwrap_or(0),
                    });
                }
            }
            commits
        }
        _ => vec![],
    }
}

/// Get the diff for a single commit (commit^ to commit).
pub fn get_commit_patch(repo_path: &Path, commit_sha: &str) -> DiffResult {
    let base = format!("{}^", commit_sha);
    let patch = get_diff_patch(repo_path, &base, commit_sha);
    let files_changed = get_files_changed(repo_path, &base, commit_sha);
    let (additions, deletions) = get_diff_stats(repo_path, &base, commit_sha);

    DiffResult {
        patch,
        files_changed,
        additions,
        deletions,
        effective_base_sha: base,
    }
}

/// Compute a diff with per-commit breakdown.
///
/// For multi-commit pushes, includes per-commit patches alongside the overall diff.
/// For single-commit pushes, `commits` has one entry and `commit_patches` is empty.
pub fn compute_diff_with_commits(
    repo_path: &Path,
    base_sha: &str,
    head_sha: &str,
) -> CommitDiffResult {
    let diff = compute_diff(repo_path, base_sha, head_sha);
    let effective_base = &diff.effective_base_sha;

    let commits = list_commits(repo_path, effective_base, head_sha);

    let commit_patches = if commits.len() > 1 {
        commits
            .iter()
            .map(|c| {
                let result = get_commit_patch(repo_path, &c.sha);
                result.patch
            })
            .collect()
    } else {
        vec![]
    };

    CommitDiffResult {
        diff,
        commits,
        commit_patches,
    }
}

/// Get the diff patch between two commits.
fn get_diff_patch(repo_path: &Path, base_sha: &str, head_sha: &str) -> String {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "diff",
            base_sha,
            head_sha,
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => String::new(),
    }
}

/// Get the list of files changed between two commits.
fn get_files_changed(repo_path: &Path, base_sha: &str, head_sha: &str) -> Vec<String> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "diff",
            "--name-only",
            base_sha,
            head_sha,
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(String::from)
            .filter(|s| !s.is_empty())
            .collect(),
        _ => vec![],
    }
}

/// Get diff statistics (additions and deletions) between two commits.
fn get_diff_stats(repo_path: &Path, base_sha: &str, head_sha: &str) -> (u32, u32) {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "diff",
            "--numstat",
            base_sha,
            head_sha,
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let mut adds = 0u32;
            let mut dels = 0u32;
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    // Handle binary files which show "-" instead of numbers
                    if let (Ok(a), Ok(d)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                        adds += a;
                        dels += d;
                    }
                }
            }
            (adds, dels)
        }
        _ => (0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_null_sha_detection() {
        // find_effective_base_sha should return the original SHA if it's not null
        let non_null = "abc123def456";
        assert_eq!(
            find_effective_base_sha(Path::new("/tmp"), non_null, "head123"),
            non_null
        );
    }

    #[test]
    fn test_empty_tree_sha_constant() {
        // Verify the empty tree SHA is the well-known constant
        assert_eq!(EMPTY_TREE_SHA, "4b825dc642cb6eb9a060e54bf8d69288fbee4904");
    }

    #[test]
    fn test_default_branches() {
        // Verify default branches are in the expected order
        assert_eq!(
            DEFAULT_BRANCHES,
            &["origin/main", "origin/master", "main", "master"]
        );
    }
}
