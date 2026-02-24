//! Path utilities for Airlock directories.

use crate::error::{AirlockError, Result};
use std::path::PathBuf;

/// The name used for the Windows named pipe.
#[cfg(windows)]
pub const WINDOWS_PIPE_NAME: &str = "airlock-daemon";

/// Provides paths to Airlock directories and files.
#[derive(Debug, Clone)]
pub struct AirlockPaths {
    /// Root directory (~/.airlock).
    root: PathBuf,
}

impl AirlockPaths {
    /// Create a new AirlockPaths instance using the default location.
    ///
    /// The location can be overridden by setting the `AIRLOCK_HOME` environment variable.
    /// If not set, defaults to `~/.airlock`.
    pub fn new() -> Result<Self> {
        // Check for AIRLOCK_HOME environment variable first
        if let Ok(custom_home) = std::env::var("AIRLOCK_HOME") {
            return Ok(Self {
                root: PathBuf::from(custom_home),
            });
        }

        let home = dirs::home_dir()
            .ok_or_else(|| AirlockError::Filesystem("Could not determine home directory".into()))?;

        Ok(Self {
            root: home.join(".airlock"),
        })
    }

    /// Create a new AirlockPaths instance with a custom root directory.
    /// Useful for testing.
    pub fn with_root(root: PathBuf) -> Self {
        Self { root }
    }

    /// Get the root directory (~/.airlock).
    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    /// Get the global config file path (~/.airlock/config.yml).
    pub fn global_config(&self) -> PathBuf {
        self.root.join("config.yml")
    }

    /// Get the state database path (~/.airlock/state.sqlite).
    pub fn database(&self) -> PathBuf {
        self.root.join("state.sqlite")
    }

    /// Get the repos directory (~/.airlock/repos).
    pub fn repos_dir(&self) -> PathBuf {
        self.root.join("repos")
    }

    /// Get the path for a specific repo's bare git directory.
    pub fn repo_gate(&self, repo_id: &str) -> PathBuf {
        self.repos_dir().join(format!("{}.git", repo_id))
    }

    /// Get the artifacts directory (~/.airlock/artifacts).
    pub fn artifacts_dir(&self) -> PathBuf {
        self.root.join("artifacts")
    }

    /// Get the artifacts directory for a specific repo.
    pub fn repo_artifacts(&self, repo_id: &str) -> PathBuf {
        self.artifacts_dir().join(repo_id)
    }

    /// Get the artifacts directory for a specific run.
    pub fn run_artifacts(&self, repo_id: &str, run_id: &str) -> PathBuf {
        self.repo_artifacts(repo_id).join(run_id)
    }

    /// Get the locks directory (~/.airlock/locks).
    pub fn locks_dir(&self) -> PathBuf {
        self.root.join("locks")
    }

    /// Get the lock file path for a specific repo.
    /// Used for synchronizing sync operations.
    pub fn repo_lock(&self, repo_id: &str) -> PathBuf {
        self.locks_dir().join(format!("{}.lock", repo_id))
    }

    /// Get the bin directory (~/.airlock/bin).
    /// Contains wrapper scripts like the upload-pack wrapper.
    pub fn bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// Get the upload-pack wrapper script path (~/.airlock/bin/airlock-upload-pack).
    pub fn upload_pack_wrapper(&self) -> PathBuf {
        self.bin_dir().join("airlock-upload-pack")
    }

    /// Get the worktrees directory (~/.airlock/worktrees).
    /// This is where temporary worktrees for intent processing are created.
    pub fn worktrees_dir(&self) -> PathBuf {
        self.root.join("worktrees")
    }

    /// Get the path for temporary worktrees used during sync rebase operations.
    ///
    /// These worktrees are short-lived and created when smart sync needs to
    /// rebase diverged branches on top of upstream.
    ///
    /// Format: `~/.airlock/worktrees/{repo_id}/sync`
    pub fn sync_worktree_dir(&self, repo_id: &str) -> PathBuf {
        self.worktrees_dir().join(repo_id).join("sync")
    }

    /// Get the path for a repo's persistent worktree.
    ///
    /// This worktree is reused across runs so that build caches
    /// (e.g. `target/`, `node_modules/`) survive between pipeline executions.
    ///
    /// Format: `~/.airlock/worktrees/{repo_id}/persistent`
    pub fn repo_worktree(&self, repo_id: &str) -> PathBuf {
        self.worktrees_dir().join(repo_id).join("persistent")
    }

    /// Get the path for a run's worktree (stage-based pipeline).
    /// Format: `~/.airlock/worktrees/{repo_id}/{run_id}`
    pub fn run_worktree(&self, repo_id: &str, run_id: &str) -> PathBuf {
        self.worktrees_dir().join(repo_id).join(run_id)
    }

    /// Get the path for a specific intent's worktree (legacy intent-centric pipeline).
    ///
    /// **DEPRECATED**: Use [`run_worktree`] for the new stage-based pipeline.
    /// This function will be removed in steps 10.13-10.16.
    ///
    /// Format: `~/.airlock/worktrees/{repo_id}/{run_id}/{intent_id}`
    #[deprecated(
        since = "0.1.0",
        note = "Part of legacy intent-centric pipeline. Use run_worktree for stage-based pipeline."
    )]
    pub fn intent_worktree(&self, repo_id: &str, run_id: &str, intent_id: &str) -> PathBuf {
        self.worktrees_dir()
            .join(repo_id)
            .join(run_id)
            .join(intent_id)
    }

    /// Get the branch name for an intent.
    ///
    /// **DEPRECATED**: This function is part of the legacy intent-centric pipeline
    /// and will be removed in steps 10.13-10.16.
    ///
    /// Format: `airlock/{run_id}/{intent_id}`
    #[deprecated(since = "0.1.0", note = "Part of legacy intent-centric pipeline.")]
    pub fn intent_branch_name(run_id: &str, intent_id: &str) -> String {
        format!("airlock/{}/{}", run_id, intent_id)
    }

    /// Get the Unix domain socket path (~/.airlock/socket).
    ///
    /// On Windows, this returns a placeholder path. Use `socket_name()` instead
    /// for the actual IPC endpoint name.
    #[cfg(unix)]
    pub fn socket(&self) -> PathBuf {
        self.root.join("socket")
    }

    /// Get a path representing the socket location (for display/logging purposes).
    ///
    /// On Windows, this returns a path under the Airlock root, but the actual
    /// IPC uses a named pipe. Use `socket_name()` for the actual endpoint.
    #[cfg(windows)]
    pub fn socket(&self) -> PathBuf {
        self.root.join("socket")
    }

    /// Get the IPC endpoint name for the current platform.
    ///
    /// On Unix, this returns the socket file path.
    /// On Windows, this returns the named pipe name.
    pub fn socket_name(&self) -> String {
        #[cfg(unix)]
        {
            self.root.join("socket").to_string_lossy().to_string()
        }
        #[cfg(windows)]
        {
            WINDOWS_PIPE_NAME.to_string()
        }
    }

    /// Ensure all required directories exist.
    pub fn ensure_dirs(&self) -> Result<()> {
        let dirs = [
            self.root.clone(),
            self.repos_dir(),
            self.artifacts_dir(),
            self.locks_dir(),
            self.worktrees_dir(),
            self.bin_dir(),
        ];

        for dir in &dirs {
            if !dir.exists() {
                std::fs::create_dir_all(dir)?;
                tracing::debug!("Created directory: {}", dir.display());
            }
        }

        Ok(())
    }
}

impl Default for AirlockPaths {
    fn default() -> Self {
        Self::new().expect("Failed to determine home directory")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_paths_with_custom_root() {
        let paths = AirlockPaths::with_root(PathBuf::from("/tmp/airlock-test"));

        assert_eq!(paths.root(), Path::new("/tmp/airlock-test"));
        assert_eq!(
            paths.database(),
            Path::new("/tmp/airlock-test/state.sqlite")
        );
        assert_eq!(paths.repos_dir(), Path::new("/tmp/airlock-test/repos"));
        assert_eq!(
            paths.repo_gate("abc123"),
            Path::new("/tmp/airlock-test/repos/abc123.git")
        );
        assert_eq!(paths.socket(), Path::new("/tmp/airlock-test/socket"));
    }

    #[test]
    fn test_run_artifacts_path() {
        let paths = AirlockPaths::with_root(PathBuf::from("/tmp/airlock-test"));

        assert_eq!(
            paths.run_artifacts("repo1", "run1"),
            Path::new("/tmp/airlock-test/artifacts/repo1/run1")
        );
    }

    #[test]
    fn test_repo_lock_path() {
        let paths = AirlockPaths::with_root(PathBuf::from("/tmp/airlock-test"));

        assert_eq!(
            paths.repo_lock("abc123"),
            Path::new("/tmp/airlock-test/locks/abc123.lock")
        );
    }

    #[test]
    fn test_airlock_home_env_var() {
        // Save original env var if it exists
        let original = std::env::var("AIRLOCK_HOME").ok();

        // Set custom AIRLOCK_HOME
        std::env::set_var("AIRLOCK_HOME", "/custom/airlock/path");

        let paths = AirlockPaths::new().expect("Should create paths with env var");
        assert_eq!(paths.root(), Path::new("/custom/airlock/path"));

        // Restore original or remove
        match original {
            Some(val) => std::env::set_var("AIRLOCK_HOME", val),
            None => std::env::remove_var("AIRLOCK_HOME"),
        }
    }

    #[test]
    fn test_worktrees_dir() {
        let paths = AirlockPaths::with_root(PathBuf::from("/tmp/airlock-test"));

        assert_eq!(
            paths.worktrees_dir(),
            Path::new("/tmp/airlock-test/worktrees")
        );
    }

    #[test]
    fn test_run_worktree_path() {
        let paths = AirlockPaths::with_root(PathBuf::from("/tmp/airlock-test"));

        assert_eq!(
            paths.run_worktree("repo-abc", "run-123"),
            Path::new("/tmp/airlock-test/worktrees/repo-abc/run-123")
        );
    }

    // Legacy tests (deprecated functions)

    #[test]
    #[allow(deprecated)]
    fn test_intent_worktree_path() {
        let paths = AirlockPaths::with_root(PathBuf::from("/tmp/airlock-test"));

        assert_eq!(
            paths.intent_worktree("repo-abc", "run-123", "intent-456"),
            Path::new("/tmp/airlock-test/worktrees/repo-abc/run-123/intent-456")
        );
    }

    #[test]
    #[allow(deprecated)]
    fn test_intent_branch_name() {
        assert_eq!(
            AirlockPaths::intent_branch_name("run-123", "intent-456"),
            "airlock/run-123/intent-456"
        );
    }
}
