//! Patch artifact command.
//!
//! Captures changes as reviewable patches for the Push Request.
//!
//! Usage:
//!   # Capture uncommitted changes (e.g., after running a linter)
//!   eslint --fix .
//!   airlock artifact patch --title "ESLint fixes" --explanation "Applied auto-fix rules"
//!
//!   # Provide diff file directly
//!   airlock artifact patch --title "Fix" --explanation "..." --diff-file fix.diff

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use tracing::{debug, info};

/// Arguments for the patch command.
#[derive(Debug)]
pub struct PatchArgs {
    /// Title for the patch.
    pub title: String,
    /// Explanation of what this patch does.
    pub explanation: String,
    /// Diff file to use instead of capturing from git.
    pub diff_file: Option<PathBuf>,
}

/// Patch artifact structure.
#[derive(Debug, Serialize)]
struct PatchArtifact {
    /// Title for this patch.
    title: String,
    /// Explanation of what this patch does.
    explanation: String,
    /// The unified diff content.
    diff: String,
}

/// Execute the patch artifact command.
///
/// Captures uncommitted changes or reads from a diff file, then:
/// 1. Writes the patch to `$AIRLOCK_ARTIFACTS/patches/<id>.json`
/// 2. Reverts the worktree to clean state (so next stage sees clean state)
pub async fn patch(args: PatchArgs) -> Result<()> {
    // Get the artifacts directory from environment
    let artifacts_dir = std::env::var("AIRLOCK_ARTIFACTS")
        .context("AIRLOCK_ARTIFACTS environment variable not set. This command must be run within a pipeline stage.")?;
    let artifacts_path = PathBuf::from(&artifacts_dir);

    // Get worktree directory for git operations
    let worktree = std::env::var("AIRLOCK_WORKTREE")
        .context("AIRLOCK_WORKTREE environment variable not set. This command must be run within a pipeline stage.")?;
    let worktree_path = PathBuf::from(&worktree);

    // Get the diff content
    let diff_content = if let Some(diff_file) = &args.diff_file {
        std::fs::read_to_string(diff_file)
            .with_context(|| format!("Failed to read diff file: {:?}", diff_file))?
    } else {
        // Capture uncommitted changes from git
        capture_git_diff(&worktree_path)?
    };

    // Check if there are any changes
    if diff_content.trim().is_empty() {
        info!("No changes to capture as patch");
        return Ok(());
    }

    // Create patches directory
    let patches_dir = artifacts_path.join("patches");
    std::fs::create_dir_all(&patches_dir)
        .with_context(|| format!("Failed to create patches directory: {:?}", patches_dir))?;

    // Generate unique ID
    let id = uuid::Uuid::new_v4().to_string();

    // Write JSON artifact
    let output_path = patches_dir.join(format!("{}.json", id));
    let artifact = PatchArtifact {
        title: args.title.clone(),
        explanation: args.explanation.clone(),
        diff: diff_content.clone(),
    };
    let json_content =
        serde_json::to_string_pretty(&artifact).context("Failed to serialize patch artifact")?;

    std::fs::write(&output_path, &json_content)
        .with_context(|| format!("Failed to write patch artifact: {:?}", output_path))?;

    info!("Created patch artifact '{}': {:?}", args.title, output_path);

    // Revert worktree to clean state (if we captured from git, not from a file)
    if args.diff_file.is_none() {
        revert_worktree(&worktree_path)?;
    }

    Ok(())
}

/// Capture uncommitted changes from git as a unified diff.
fn capture_git_diff(worktree: &PathBuf) -> Result<String> {
    // Stage all changes first to include new files
    let stage_output = Command::new("git")
        .args(["add", "-A"])
        .current_dir(worktree)
        .output()
        .context("Failed to execute git add")?;

    if !stage_output.status.success() {
        let stderr = String::from_utf8_lossy(&stage_output.stderr);
        debug!("git add warning: {}", stderr);
    }

    // Get diff of staged changes against HEAD
    let diff_output = Command::new("git")
        .args(["diff", "--cached", "--no-color"])
        .current_dir(worktree)
        .output()
        .context("Failed to execute git diff")?;

    if !diff_output.status.success() {
        let stderr = String::from_utf8_lossy(&diff_output.stderr);
        anyhow::bail!("git diff failed: {}", stderr);
    }

    let diff = String::from_utf8_lossy(&diff_output.stdout).to_string();
    Ok(diff)
}

/// Revert worktree to clean state.
fn revert_worktree(worktree: &PathBuf) -> Result<()> {
    // First reset the staging area
    let reset_output = Command::new("git")
        .args(["reset", "HEAD", "--quiet"])
        .current_dir(worktree)
        .output()
        .context("Failed to execute git reset")?;

    if !reset_output.status.success() {
        let stderr = String::from_utf8_lossy(&reset_output.stderr);
        debug!("git reset warning: {}", stderr);
    }

    // Then discard all changes
    let checkout_output = Command::new("git")
        .args(["checkout", "--", "."])
        .current_dir(worktree)
        .output()
        .context("Failed to execute git checkout")?;

    if !checkout_output.status.success() {
        let stderr = String::from_utf8_lossy(&checkout_output.stderr);
        anyhow::bail!("git checkout failed: {}", stderr);
    }

    // Clean untracked files
    let clean_output = Command::new("git")
        .args(["clean", "-fd"])
        .current_dir(worktree)
        .output()
        .context("Failed to execute git clean")?;

    if !clean_output.status.success() {
        let stderr = String::from_utf8_lossy(&clean_output.stderr);
        debug!("git clean warning: {}", stderr);
    }

    info!("Reverted worktree to clean state");
    Ok(())
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
        std::fs::write(repo_path.join("file.txt"), "initial content").unwrap();
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
    async fn test_patch_captures_changes() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = setup_git_repo(&temp_dir);
        let artifacts_dir = temp_dir.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        std::env::set_var("AIRLOCK_ARTIFACTS", artifacts_dir.to_str().unwrap());
        std::env::set_var("AIRLOCK_WORKTREE", repo_path.to_str().unwrap());

        // Make a change
        std::fs::write(repo_path.join("file.txt"), "modified content").unwrap();

        let args = PatchArgs {
            title: "Test Patch".to_string(),
            explanation: "This is a test patch".to_string(),
            diff_file: None,
        };

        patch(args).await.unwrap();

        // Verify patches directory was created
        let patches_dir = artifacts_dir.join("patches");
        assert!(patches_dir.exists());

        // Verify file was created
        let files: Vec<_> = std::fs::read_dir(&patches_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1);

        // Verify content
        let file_content = std::fs::read_to_string(files[0].path()).unwrap();
        let artifact: serde_json::Value = serde_json::from_str(&file_content).unwrap();
        assert_eq!(artifact["title"], "Test Patch");
        assert!(artifact["diff"]
            .as_str()
            .unwrap()
            .contains("modified content"));

        // Verify worktree was reverted
        let file_content = std::fs::read_to_string(repo_path.join("file.txt")).unwrap();
        assert_eq!(file_content, "initial content");

        std::env::remove_var("AIRLOCK_ARTIFACTS");
        std::env::remove_var("AIRLOCK_WORKTREE");
    }

    #[tokio::test]
    #[serial]
    async fn test_patch_from_diff_file() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = setup_git_repo(&temp_dir);
        let artifacts_dir = temp_dir.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        std::env::set_var("AIRLOCK_ARTIFACTS", artifacts_dir.to_str().unwrap());
        std::env::set_var("AIRLOCK_WORKTREE", repo_path.to_str().unwrap());

        // Create diff file
        let diff_file = temp_dir.path().join("test.diff");
        std::fs::write(
            &diff_file,
            "--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-old\n+new\n",
        )
        .unwrap();

        let args = PatchArgs {
            title: "External Patch".to_string(),
            explanation: "From diff file".to_string(),
            diff_file: Some(diff_file),
        };

        patch(args).await.unwrap();

        // Verify patches directory was created
        let patches_dir = artifacts_dir.join("patches");
        let files: Vec<_> = std::fs::read_dir(&patches_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1);

        let file_content = std::fs::read_to_string(files[0].path()).unwrap();
        let artifact: serde_json::Value = serde_json::from_str(&file_content).unwrap();
        assert_eq!(artifact["title"], "External Patch");
        assert!(artifact["diff"].as_str().unwrap().contains("-old"));

        std::env::remove_var("AIRLOCK_ARTIFACTS");
        std::env::remove_var("AIRLOCK_WORKTREE");
    }

    #[tokio::test]
    #[serial]
    async fn test_patch_no_changes() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = setup_git_repo(&temp_dir);
        let artifacts_dir = temp_dir.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        std::env::set_var("AIRLOCK_ARTIFACTS", artifacts_dir.to_str().unwrap());
        std::env::set_var("AIRLOCK_WORKTREE", repo_path.to_str().unwrap());

        // No changes made

        let args = PatchArgs {
            title: "Empty Patch".to_string(),
            explanation: "No changes".to_string(),
            diff_file: None,
        };

        patch(args).await.unwrap();

        // Verify no patches created (no changes)
        let patches_dir = artifacts_dir.join("patches");
        if patches_dir.exists() {
            let files: Vec<_> = std::fs::read_dir(&patches_dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .collect();
            assert_eq!(files.len(), 0);
        }

        std::env::remove_var("AIRLOCK_ARTIFACTS");
        std::env::remove_var("AIRLOCK_WORKTREE");
    }
}
