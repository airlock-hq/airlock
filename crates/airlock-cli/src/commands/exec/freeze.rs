//! Freeze stage implementation - apply patches and create a commit checkpoint.
//!
//! The freeze command:
//! 1. Reads all patches from `$AIRLOCK_ARTIFACTS/patches/`
//! 2. Applies each patch to the worktree
//! 3. Stages all changes and creates a commit
//! 4. Writes the new commit SHA to `$AIRLOCK_ARTIFACTS/.head_sha`
//!
//! This separates pre-freeze stages (patches auto-applied) from post-freeze stages
//! (patches queued for review).
//!
//! Usage:
//!   airlock exec freeze

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;
use tracing::{debug, info, warn};

/// Patch artifact structure (must match artifact/patch.rs).
#[derive(Debug, Deserialize)]
struct PatchArtifact {
    /// Title for this patch.
    title: String,
    /// The unified diff content.
    diff: String,
}

/// Execute the `freeze` command.
///
/// Applies all pending patches and creates a checkpoint commit.
pub async fn freeze() -> Result<()> {
    info!("Executing freeze stage...");

    // Get required environment variables
    let artifacts_dir = std::env::var("AIRLOCK_ARTIFACTS").context(
        "AIRLOCK_ARTIFACTS environment variable not set. This command must be run within a pipeline stage.",
    )?;
    let artifacts_path = PathBuf::from(&artifacts_dir);

    let worktree = std::env::var("AIRLOCK_WORKTREE").context(
        "AIRLOCK_WORKTREE environment variable not set. This command must be run within a pipeline stage.",
    )?;
    let worktree_path = PathBuf::from(&worktree);

    // Check if patches directory exists
    let patches_dir = artifacts_path.join("patches");
    if !patches_dir.exists() {
        info!("No patches directory found, nothing to freeze");
        return Ok(());
    }

    // Read and apply patches
    let patches = read_patches(&patches_dir)?;
    if patches.is_empty() {
        info!("No patches found, nothing to freeze");
        return Ok(());
    }

    info!("Found {} patches to apply", patches.len());

    // Apply each patch
    let mut applied_titles = Vec::new();
    for (path, patch) in &patches {
        debug!("Applying patch '{}' from {:?}", patch.title, path);

        if patch.diff.trim().is_empty() {
            debug!("Skipping empty patch '{}'", patch.title);
            continue;
        }

        apply_patch(&worktree_path, &patch.diff)
            .with_context(|| format!("Failed to apply patch '{}' from {:?}", patch.title, path))?;

        applied_titles.push(patch.title.clone());
        info!("Applied patch: {}", patch.title);
    }

    if applied_titles.is_empty() {
        info!("All patches were empty, nothing to commit");
        return Ok(());
    }

    // Stage all changes
    stage_all_changes(&worktree_path)?;

    // Check if there are changes to commit
    if !has_staged_changes(&worktree_path)? {
        info!("No changes to commit after applying patches");
        return Ok(());
    }

    // Create commit
    let commit_message = format!("Airlock: auto-fixes from {}", applied_titles.join(", "));
    let new_sha = create_commit(&worktree_path, &commit_message)?;

    info!(
        "Created freeze commit: {}",
        &new_sha[..12.min(new_sha.len())]
    );

    // Write new SHA to artifacts
    let head_sha_path = artifacts_path.join(".head_sha");
    std::fs::write(&head_sha_path, &new_sha)
        .with_context(|| format!("Failed to write .head_sha to {:?}", head_sha_path))?;

    info!(
        "Freeze complete. New HEAD: {}",
        &new_sha[..12.min(new_sha.len())]
    );

    Ok(())
}

/// Read all patches from the patches directory.
fn read_patches(patches_dir: &PathBuf) -> Result<Vec<(PathBuf, PatchArtifact)>> {
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
fn apply_patch(worktree: &PathBuf, diff: &str) -> Result<()> {
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
fn stage_all_changes(worktree: &PathBuf) -> Result<()> {
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
fn has_staged_changes(worktree: &PathBuf) -> Result<bool> {
    let output = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(worktree)
        .output()
        .context("Failed to execute git diff --cached")?;

    // Exit code 0 = no changes, 1 = changes exist
    Ok(!output.status.success())
}

/// Create a commit with the given message.
fn create_commit(worktree: &PathBuf, message: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(worktree)
        .output()
        .context("Failed to execute git commit")?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    fn setup_git_repo(temp_dir: &TempDir) -> PathBuf {
        let repo_path = temp_dir.path().join("repo");
        std::fs::create_dir_all(&repo_path).unwrap();

        // Initialize git repo
        Command::new("git")
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Configure git user
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

        // Create initial commit
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

    #[tokio::test]
    #[serial]
    async fn test_freeze_no_patches() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = setup_git_repo(&temp_dir);
        let artifacts_dir = temp_dir.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        std::env::set_var("AIRLOCK_ARTIFACTS", artifacts_dir.to_str().unwrap());
        std::env::set_var("AIRLOCK_WORKTREE", repo_path.to_str().unwrap());

        // No patches directory
        freeze().await.unwrap();

        // Verify no .head_sha was created
        assert!(!artifacts_dir.join(".head_sha").exists());

        std::env::remove_var("AIRLOCK_ARTIFACTS");
        std::env::remove_var("AIRLOCK_WORKTREE");
    }

    #[tokio::test]
    #[serial]
    async fn test_freeze_with_patch() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = setup_git_repo(&temp_dir);
        let artifacts_dir = temp_dir.path().join("artifacts");
        let patches_dir = artifacts_dir.join("patches");
        std::fs::create_dir_all(&patches_dir).unwrap();

        std::env::set_var("AIRLOCK_ARTIFACTS", artifacts_dir.to_str().unwrap());
        std::env::set_var("AIRLOCK_WORKTREE", repo_path.to_str().unwrap());

        // Create a patch
        let patch = r#"{
            "title": "Test fix",
            "explanation": "A test fix",
            "diff": "--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-initial content\n+modified content\n"
        }"#;
        std::fs::write(patches_dir.join("patch1.json"), patch).unwrap();

        freeze().await.unwrap();

        // Verify .head_sha was created
        assert!(artifacts_dir.join(".head_sha").exists());

        // Verify the file was modified
        let content = std::fs::read_to_string(repo_path.join("file.txt")).unwrap();
        assert_eq!(content, "modified content\n");

        // Verify commit was created
        let log_output = Command::new("git")
            .args(["log", "--oneline", "-1"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&log_output.stdout);
        assert!(log.contains("Airlock: auto-fixes from Test fix"));

        std::env::remove_var("AIRLOCK_ARTIFACTS");
        std::env::remove_var("AIRLOCK_WORKTREE");
    }

    #[tokio::test]
    #[serial]
    async fn test_freeze_empty_patch() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = setup_git_repo(&temp_dir);
        let artifacts_dir = temp_dir.path().join("artifacts");
        let patches_dir = artifacts_dir.join("patches");
        std::fs::create_dir_all(&patches_dir).unwrap();

        std::env::set_var("AIRLOCK_ARTIFACTS", artifacts_dir.to_str().unwrap());
        std::env::set_var("AIRLOCK_WORKTREE", repo_path.to_str().unwrap());

        // Create an empty patch
        let patch = r#"{
            "title": "Empty fix",
            "explanation": "Nothing here",
            "diff": ""
        }"#;
        std::fs::write(patches_dir.join("empty.json"), patch).unwrap();

        freeze().await.unwrap();

        // Verify no .head_sha was created (no actual changes)
        assert!(!artifacts_dir.join(".head_sha").exists());

        std::env::remove_var("AIRLOCK_ARTIFACTS");
        std::env::remove_var("AIRLOCK_WORKTREE");
    }

    #[tokio::test]
    #[serial]
    async fn test_freeze_multiple_patches() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = setup_git_repo(&temp_dir);
        let artifacts_dir = temp_dir.path().join("artifacts");
        let patches_dir = artifacts_dir.join("patches");
        std::fs::create_dir_all(&patches_dir).unwrap();

        // Create second file for second patch
        std::fs::write(repo_path.join("other.txt"), "other content\n").unwrap();
        Command::new("git")
            .args(["add", "other.txt"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "add other.txt"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        std::env::set_var("AIRLOCK_ARTIFACTS", artifacts_dir.to_str().unwrap());
        std::env::set_var("AIRLOCK_WORKTREE", repo_path.to_str().unwrap());

        // Create patches
        let patch1 = r#"{
            "title": "Fix 1",
            "explanation": "First fix",
            "diff": "--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-initial content\n+fixed content\n"
        }"#;
        let patch2 = r#"{
            "title": "Fix 2",
            "explanation": "Second fix",
            "diff": "--- a/other.txt\n+++ b/other.txt\n@@ -1 +1 @@\n-other content\n+other fixed\n"
        }"#;
        std::fs::write(patches_dir.join("01-patch1.json"), patch1).unwrap();
        std::fs::write(patches_dir.join("02-patch2.json"), patch2).unwrap();

        freeze().await.unwrap();

        // Verify both files were modified
        let content1 = std::fs::read_to_string(repo_path.join("file.txt")).unwrap();
        assert_eq!(content1, "fixed content\n");

        let content2 = std::fs::read_to_string(repo_path.join("other.txt")).unwrap();
        assert_eq!(content2, "other fixed\n");

        // Verify commit message mentions both fixes
        let log_output = Command::new("git")
            .args(["log", "--oneline", "-1"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&log_output.stdout);
        assert!(log.contains("Airlock: auto-fixes from Fix 1, Fix 2"));

        std::env::remove_var("AIRLOCK_ARTIFACTS");
        std::env::remove_var("AIRLOCK_WORKTREE");
    }
}
