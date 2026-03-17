//! Patch application utilities.
//!
//! Shared logic for reading, applying, and committing patches produced by
//! pipeline steps.  Used by both `airlock exec freeze` (CLI) and the
//! `apply-patch` executor integration (daemon).

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info, warn};

/// Patch artifact structure (must match artifact/patch.rs).
#[derive(Debug, Deserialize)]
pub struct PatchArtifact {
    /// Title for this patch.
    pub title: String,
    /// The unified diff content.
    pub diff: String,
}

/// Read all patches from the patches directory.
///
/// Returns patches sorted by filename for deterministic ordering.
/// Only reads top-level `.json` files (skips the `applied/` subdirectory).
pub fn read_patches(patches_dir: &Path) -> Result<Vec<(PathBuf, PatchArtifact)>> {
    let mut patches = Vec::new();

    let entries = std::fs::read_dir(patches_dir)
        .with_context(|| format!("Failed to read patches directory: {:?}", patches_dir))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        // Skip non-JSON files
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read patch file: {:?}", path))?;

        let patch: PatchArtifact = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse patch file: {:?}", path))?;

        patches.push((path, patch));
    }

    // Sort by filename for deterministic order
    patches.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(patches)
}

/// Apply a unified diff patch to the worktree.
pub fn apply_patch(worktree: &Path, diff: &str) -> Result<()> {
    // Write diff to a temporary file
    let temp_dir = std::env::temp_dir();
    let temp_patch = temp_dir.join(format!("airlock-patch-{}.diff", uuid::Uuid::new_v4()));

    std::fs::write(&temp_patch, diff)
        .with_context(|| format!("Failed to write temporary patch file: {:?}", temp_patch))?;

    // Apply the patch using git apply
    let output = Command::new("git")
        .args(["apply", "--3way"])
        .arg(&temp_patch)
        .current_dir(worktree)
        .output()
        .context("Failed to execute git apply")?;

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_patch);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Try without --3way for simpler patches
        debug!("git apply --3way failed, trying without --3way: {}", stderr);

        let temp_patch2 = temp_dir.join(format!("airlock-patch-{}.diff", uuid::Uuid::new_v4()));
        std::fs::write(&temp_patch2, diff)?;

        let output2 = Command::new("git")
            .args(["apply"])
            .arg(&temp_patch2)
            .current_dir(worktree)
            .output()
            .context("Failed to execute git apply")?;

        let _ = std::fs::remove_file(&temp_patch2);

        if !output2.status.success() {
            let stderr2 = String::from_utf8_lossy(&output2.stderr);
            anyhow::bail!("git apply failed: {}", stderr2);
        }
    }

    Ok(())
}

/// Stage all changes in the worktree.
pub fn stage_all_changes(worktree: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["add", "-A"])
        .current_dir(worktree)
        .output()
        .context("Failed to execute git add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("git add warning: {}", stderr);
    }

    Ok(())
}

/// Check if there are staged changes to commit.
pub fn has_staged_changes(worktree: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(worktree)
        .output()
        .context("Failed to execute git diff --cached")?;

    // Exit code 0 = no changes, 1 = changes exist
    Ok(!output.status.success())
}

/// Create a commit with the given message.
///
/// When `author_name`/`author_email` are provided they are set as
/// `GIT_AUTHOR_*` so the commit is attributed to the original user.
/// `GIT_COMMITTER_*` is always set to Airlock.
pub fn create_commit(
    worktree: &Path,
    message: &str,
    author_name: Option<&str>,
    author_email: Option<&str>,
) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(["commit", "-m", message]).current_dir(worktree);

    // Committer is always Airlock.
    cmd.env("GIT_COMMITTER_NAME", "Airlock");
    cmd.env("GIT_COMMITTER_EMAIL", "airlock@airlockhq.com");

    if let Some(name) = author_name {
        cmd.env("GIT_AUTHOR_NAME", name);
    }
    if let Some(email) = author_email {
        cmd.env("GIT_AUTHOR_EMAIL", email);
    }

    let output = cmd.output().context("Failed to execute git commit")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git commit failed: {}", stderr);
    }

    // Get the new HEAD SHA
    let sha_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(worktree)
        .output()
        .context("Failed to execute git rev-parse HEAD")?;

    if !sha_output.status.success() {
        let stderr = String::from_utf8_lossy(&sha_output.stderr);
        anyhow::bail!("git rev-parse HEAD failed: {}", stderr);
    }

    let sha = String::from_utf8_lossy(&sha_output.stdout)
        .trim()
        .to_string();

    Ok(sha)
}

/// Apply all pending patches in `artifacts_dir/patches/`, commit them, and
/// return the new HEAD SHA.
///
/// Returns `Ok(Some(new_sha))` if patches were applied, `Ok(None)` if there
/// was nothing to apply.
pub fn apply_pending_patches(
    worktree: &Path,
    artifacts_dir: &Path,
    author_name: Option<&str>,
    author_email: Option<&str>,
) -> Result<Option<String>> {
    let patches_dir = artifacts_dir.join("patches");
    if !patches_dir.exists() {
        return Ok(None);
    }

    let patches = read_patches(&patches_dir)?;
    if patches.is_empty() {
        return Ok(None);
    }

    info!("apply-patch: found {} patches to apply", patches.len());

    // Create applied directory for moving patches after successful apply
    let applied_dir = patches_dir.join("applied");
    std::fs::create_dir_all(&applied_dir).with_context(|| {
        format!(
            "Failed to create applied patches directory: {:?}",
            applied_dir
        )
    })?;

    let mut applied_titles = Vec::new();
    for (path, patch) in &patches {
        debug!("Applying patch '{}' from {:?}", patch.title, path);

        if patch.diff.trim().is_empty() {
            debug!("Skipping empty patch '{}'", patch.title);
            continue;
        }

        apply_patch(worktree, &patch.diff)
            .with_context(|| format!("Failed to apply patch '{}' from {:?}", patch.title, path))?;

        // Move applied patch to patches/applied/
        if let Some(filename) = path.file_name() {
            let dest = applied_dir.join(filename);
            std::fs::rename(path, &dest).with_context(|| {
                format!("Failed to move applied patch {:?} to {:?}", path, dest)
            })?;
            debug!("Moved applied patch to {:?}", dest);
        }

        applied_titles.push(patch.title.clone());
        info!("Applied patch: {}", patch.title);
    }

    if applied_titles.is_empty() {
        return Ok(None);
    }

    // Stage all changes
    stage_all_changes(worktree)?;

    // Check if there are changes to commit
    if !has_staged_changes(worktree)? {
        info!("No changes to commit after applying patches");
        return Ok(None);
    }

    // Create commit
    let commit_message = format!("Airlock: auto-fixes from {}", applied_titles.join(", "));
    let new_sha = create_commit(worktree, &commit_message, author_name, author_email)?;

    info!("apply-patch commit: {}", &new_sha[..12.min(new_sha.len())]);

    Ok(Some(new_sha))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_git_repo(temp_dir: &TempDir) -> PathBuf {
        let repo_path = temp_dir.path().join("repo");
        std::fs::create_dir_all(&repo_path).unwrap();

        Command::new("git")
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        std::fs::write(repo_path.join("file.txt"), "initial content\n").unwrap();
        Command::new("git")
            .args(["add", "file.txt"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        repo_path
    }

    #[test]
    fn test_apply_pending_patches_no_patches_dir() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = setup_git_repo(&temp_dir);
        let artifacts_dir = temp_dir.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        let result = apply_pending_patches(&repo_path, &artifacts_dir, None, None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_apply_pending_patches_with_patch() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = setup_git_repo(&temp_dir);
        let artifacts_dir = temp_dir.path().join("artifacts");
        let patches_dir = artifacts_dir.join("patches");
        std::fs::create_dir_all(&patches_dir).unwrap();

        let patch = r#"{
            "title": "Test fix",
            "diff": "--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-initial content\n+modified content\n"
        }"#;
        std::fs::write(patches_dir.join("patch1.json"), patch).unwrap();

        let result = apply_pending_patches(&repo_path, &artifacts_dir, None, None).unwrap();
        assert!(result.is_some());

        let content = std::fs::read_to_string(repo_path.join("file.txt")).unwrap();
        assert_eq!(content, "modified content\n");

        // Patch should be moved to applied/
        assert!(!patches_dir.join("patch1.json").exists());
        assert!(patches_dir.join("applied").join("patch1.json").exists());
    }

    #[test]
    fn test_create_commit_sets_author_and_committer_identity() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = setup_git_repo(&temp_dir);

        // Make a change to commit
        std::fs::write(repo_path.join("file.txt"), "changed\n").unwrap();
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        let sha = create_commit(
            &repo_path,
            "test commit",
            Some("Alice User"),
            Some("alice@example.com"),
        )
        .unwrap();
        assert!(!sha.is_empty());

        // Verify author is Alice (from working repo)
        let log = Command::new("git")
            .args(["log", "-1", "--format=%an <%ae>"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        let author = String::from_utf8_lossy(&log.stdout).trim().to_string();
        assert_eq!(author, "Alice User <alice@example.com>");

        // Verify committer is Airlock
        let log = Command::new("git")
            .args(["log", "-1", "--format=%cn <%ce>"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        let committer = String::from_utf8_lossy(&log.stdout).trim().to_string();
        assert_eq!(committer, "Airlock <airlock@airlockhq.com>");
    }

    #[test]
    fn test_create_commit_without_author_uses_committer_as_fallback() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = setup_git_repo(&temp_dir);

        // Make a change to commit
        std::fs::write(repo_path.join("file.txt"), "changed\n").unwrap();
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        let sha = create_commit(&repo_path, "test commit", None, None).unwrap();
        assert!(!sha.is_empty());

        // Verify committer is Airlock
        let log = Command::new("git")
            .args(["log", "-1", "--format=%cn <%ce>"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        let committer = String::from_utf8_lossy(&log.stdout).trim().to_string();
        assert_eq!(committer, "Airlock <airlock@airlockhq.com>");
    }

    #[test]
    fn test_apply_pending_patches_sets_author_identity() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = setup_git_repo(&temp_dir);
        let artifacts_dir = temp_dir.path().join("artifacts");
        let patches_dir = artifacts_dir.join("patches");
        std::fs::create_dir_all(&patches_dir).unwrap();

        let patch = r#"{
            "title": "Test fix",
            "diff": "--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-initial content\n+modified content\n"
        }"#;
        std::fs::write(patches_dir.join("patch1.json"), patch).unwrap();

        let result = apply_pending_patches(
            &repo_path,
            &artifacts_dir,
            Some("Bob Dev"),
            Some("bob@example.com"),
        )
        .unwrap();
        assert!(result.is_some());

        // Verify author is Bob
        let log = Command::new("git")
            .args(["log", "-1", "--format=%an <%ae>"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        let author = String::from_utf8_lossy(&log.stdout).trim().to_string();
        assert_eq!(author, "Bob Dev <bob@example.com>");

        // Verify committer is Airlock
        let log = Command::new("git")
            .args(["log", "-1", "--format=%cn <%ce>"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        let committer = String::from_utf8_lossy(&log.stdout).trim().to_string();
        assert_eq!(committer, "Airlock <airlock@airlockhq.com>");
    }

    #[test]
    fn test_apply_pending_patches_empty_diff() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = setup_git_repo(&temp_dir);
        let artifacts_dir = temp_dir.path().join("artifacts");
        let patches_dir = artifacts_dir.join("patches");
        std::fs::create_dir_all(&patches_dir).unwrap();

        let patch = r#"{"title": "Empty fix", "diff": ""}"#;
        std::fs::write(patches_dir.join("empty.json"), patch).unwrap();

        let result = apply_pending_patches(&repo_path, &artifacts_dir, None, None).unwrap();
        assert!(result.is_none());
    }
}
