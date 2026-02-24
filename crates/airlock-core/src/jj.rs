//! Jujutsu (jj) colocated repository support.
//!
//! When a repository uses jj colocated with git (`.jj/` alongside `.git/`),
//! jj maintains its own bookmark tracking state separate from git's remote
//! tracking configuration. After Airlock rewires git remotes during init/eject,
//! we need to synchronize jj's bookmark tracking to match.
//!
//! All jj operations are **best-effort** — failures produce warnings but never
//! block init or eject.

use crate::error::{AirlockError, Result};
use std::path::Path;
use std::process::Command;

/// Check if a working directory is a jj colocated repository.
///
/// Returns true if a `.jj` directory exists alongside the git repository.
pub fn is_colocated(working_path: &Path) -> bool {
    working_path.join(".jj").is_dir()
}

/// Check if the `jj` binary is available on PATH.
pub fn is_available() -> bool {
    Command::new("jj")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run `jj git import` to sync git ref changes into jj.
///
/// After git-level remote rewiring, jj doesn't automatically see the changes.
/// This imports the new git refs into jj's model.
pub fn git_import(working_path: &Path) -> Result<()> {
    tracing::debug!("Running jj git import in {}", working_path.display());

    let output = Command::new("jj")
        .args(["git", "import"])
        .current_dir(working_path)
        .output()
        .map_err(|e| AirlockError::Git(format!("Failed to execute jj git import: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AirlockError::Git(format!(
            "jj git import failed: {}",
            stderr.trim()
        )));
    }

    tracing::debug!("jj git import completed");
    Ok(())
}

/// Track all bookmarks from a remote in jj.
///
/// Runs `jj bookmark track 'glob:*@<remote>'` to tell jj to track
/// all bookmarks from the specified remote.
pub fn track_bookmarks(working_path: &Path, remote: &str) -> Result<()> {
    tracing::debug!(
        "Tracking jj bookmarks for remote '{}' in {}",
        remote,
        working_path.display()
    );

    let glob = format!("glob:*@{}", remote);
    let output = Command::new("jj")
        .args(["bookmark", "track", &glob])
        .current_dir(working_path)
        .output()
        .map_err(|e| AirlockError::Git(format!("Failed to execute jj bookmark track: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AirlockError::Git(format!(
            "jj bookmark track failed: {}",
            stderr.trim()
        )));
    }

    tracing::debug!("Tracked bookmarks for remote '{}'", remote);
    Ok(())
}

/// Untrack all bookmarks from a remote in jj.
///
/// Runs `jj bookmark untrack 'glob:*@<remote>'` to tell jj to stop
/// tracking bookmarks from the specified remote.
pub fn untrack_bookmarks(working_path: &Path, remote: &str) -> Result<()> {
    tracing::debug!(
        "Untracking jj bookmarks for remote '{}' in {}",
        remote,
        working_path.display()
    );

    let glob = format!("glob:*@{}", remote);
    let output = Command::new("jj")
        .args(["bookmark", "untrack", &glob])
        .current_dir(working_path)
        .output()
        .map_err(|e| AirlockError::Git(format!("Failed to execute jj bookmark untrack: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AirlockError::Git(format!(
            "jj bookmark untrack failed: {}",
            stderr.trim()
        )));
    }

    tracing::debug!("Untracked bookmarks for remote '{}'", remote);
    Ok(())
}

/// Synchronize jj bookmark tracking after `airlock init`.
///
/// After init, git remotes are rewired:
///   - `origin` → local gate
///   - `upstream` → real remote (e.g., GitHub)
///
/// We need jj to:
///   1. Import the git ref changes (`jj git import`)
///   2. Track bookmarks from `origin` (gate)
///   3. Untrack bookmarks from `upstream` (so jj doesn't show them as tracked)
pub fn sync_after_init(working_path: &Path) -> Result<()> {
    tracing::debug!(
        "Synchronizing jj bookmarks after init in {}",
        working_path.display()
    );

    git_import(working_path)?;
    track_bookmarks(working_path, "origin")?;

    // Untracking upstream bookmarks may fail if upstream has no bookmarks yet
    // (e.g., fresh repo). That's fine — log and continue.
    if let Err(e) = untrack_bookmarks(working_path, "upstream") {
        tracing::warn!("Failed to untrack upstream bookmarks (non-fatal): {}", e);
    }

    tracing::debug!("jj bookmark sync after init completed");
    Ok(())
}

/// Synchronize jj bookmark tracking after `airlock eject`.
///
/// After eject, git remotes are restored:
///   - `origin` → real remote (e.g., GitHub)
///   - `upstream` is removed
///
/// We need jj to:
///   1. Import the git ref changes (`jj git import`)
///   2. Track bookmarks from `origin` (now the real remote again)
pub fn sync_after_eject(working_path: &Path) -> Result<()> {
    tracing::debug!(
        "Synchronizing jj bookmarks after eject in {}",
        working_path.display()
    );

    git_import(working_path)?;
    track_bookmarks(working_path, "origin")?;

    tracing::debug!("jj bookmark sync after eject completed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_is_colocated_with_jj_dir() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path();

        // Create .jj and .git directories
        fs::create_dir(path.join(".jj")).unwrap();
        fs::create_dir(path.join(".git")).unwrap();

        assert!(is_colocated(path));
    }

    #[test]
    fn test_is_colocated_without_jj_dir() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path();

        // Only create .git directory
        fs::create_dir(path.join(".git")).unwrap();

        assert!(!is_colocated(path));
    }

    #[test]
    fn test_is_colocated_with_jj_file_not_dir() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path();

        // Create .jj as a file (not a directory) — should not count
        fs::write(path.join(".jj"), "").unwrap();
        fs::create_dir(path.join(".git")).unwrap();

        assert!(!is_colocated(path));
    }

    #[test]
    fn test_is_colocated_empty_dir() {
        let temp_dir = TempDir::new().unwrap();
        assert!(!is_colocated(temp_dir.path()));
    }

    // Integration tests that require jj to be installed.
    // These are guarded with an early return if jj is not available.

    #[test]
    fn test_sync_after_init_with_jj() {
        if !is_available() {
            eprintln!("Skipping test: jj not installed");
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path();

        // Create a jj colocated repo
        let output = Command::new("jj")
            .args(["git", "init", "--colocate"])
            .current_dir(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "jj git init --colocate failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        // Create a bare remote to simulate upstream
        let remote_dir = temp_dir.path().join("remote.git");
        let output = Command::new("git")
            .args(["init", "--bare", remote_dir.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(output.status.success());

        // Add origin pointing to remote
        let output = Command::new("git")
            .args([
                "-C",
                path.to_str().unwrap(),
                "remote",
                "add",
                "origin",
                remote_dir.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(output.status.success());

        // Simulate init remote rewiring: rename origin → upstream, add new origin (gate)
        let gate_dir = temp_dir.path().join("gate.git");
        let output = Command::new("git")
            .args(["init", "--bare", gate_dir.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(output.status.success());

        let output = Command::new("git")
            .args([
                "-C",
                path.to_str().unwrap(),
                "remote",
                "rename",
                "origin",
                "upstream",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());

        let output = Command::new("git")
            .args([
                "-C",
                path.to_str().unwrap(),
                "remote",
                "add",
                "origin",
                gate_dir.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(output.status.success());

        // Run sync_after_init
        let result = sync_after_init(path);
        assert!(result.is_ok(), "sync_after_init failed: {:?}", result.err());
    }

    #[test]
    fn test_sync_after_eject_with_jj() {
        if !is_available() {
            eprintln!("Skipping test: jj not installed");
            return;
        }

        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path();

        // Create a jj colocated repo
        let output = Command::new("jj")
            .args(["git", "init", "--colocate"])
            .current_dir(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "jj git init --colocate failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        // Create a bare remote to simulate the real upstream
        let remote_dir = temp_dir.path().join("remote.git");
        let output = Command::new("git")
            .args(["init", "--bare", remote_dir.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(output.status.success());

        // Add origin pointing to remote (simulating post-eject state)
        let output = Command::new("git")
            .args([
                "-C",
                path.to_str().unwrap(),
                "remote",
                "add",
                "origin",
                remote_dir.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(output.status.success());

        // Run sync_after_eject
        let result = sync_after_eject(path);
        assert!(
            result.is_ok(),
            "sync_after_eject failed: {:?}",
            result.err()
        );
    }
}
