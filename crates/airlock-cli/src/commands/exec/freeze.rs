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
use std::path::PathBuf;
use tracing::info;

/// Execute the `freeze` command.
///
/// Applies all pending patches and creates a checkpoint commit.
pub async fn freeze() -> Result<()> {
    info!("Executing freeze stage...");

    let artifacts_dir = std::env::var("AIRLOCK_ARTIFACTS").context(
        "AIRLOCK_ARTIFACTS environment variable not set. This command must be run within a pipeline stage.",
    )?;
    let artifacts_path = PathBuf::from(&artifacts_dir);

    let worktree = std::env::var("AIRLOCK_WORKTREE").context(
        "AIRLOCK_WORKTREE environment variable not set. This command must be run within a pipeline stage.",
    )?;
    let worktree_path = PathBuf::from(&worktree);

    // Author/committer env vars are inherited from the stage environment
    // (set by StageEnvironment::to_env_vars in the daemon).
    match airlock_core::patches::apply_pending_patches(&worktree_path, &artifacts_path, None, None)?
    {
        Some(new_sha) => {
            // Write new SHA to artifacts
            let head_sha_path = artifacts_path.join(".head_sha");
            std::fs::write(&head_sha_path, &new_sha)
                .with_context(|| format!("Failed to write .head_sha to {:?}", head_sha_path))?;

            info!(
                "Freeze complete. New HEAD: {}",
                &new_sha[..12.min(new_sha.len())]
            );
        }
        None => {
            info!("No patches to apply, nothing to freeze");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::process::Command;
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
