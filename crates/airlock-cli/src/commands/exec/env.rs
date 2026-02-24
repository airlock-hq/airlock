//! Execution environment for pipeline stages.

use anyhow::{Context, Result};
use std::env;
use std::path::PathBuf;

/// Environment variables read from the stage executor.
///
/// These are set by the pipeline executor before running stage commands.
/// See `airlock-daemon/src/pipeline/executor.rs` for the source.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ExecEnvironment {
    /// Unique run identifier (UUID).
    pub run_id: String,
    /// Branch being pushed (e.g., "refs/heads/feature/add-auth").
    pub branch: String,
    /// Base commit SHA (before push).
    pub base_sha: String,
    /// Head commit SHA (after push).
    pub head_sha: String,
    /// Absolute path to run worktree (also CWD).
    pub worktree: PathBuf,
    /// Directory for run-level artifacts (shared by all stages).
    /// This is where stages should write their output files like description.json, pr_result.json.
    pub artifacts: PathBuf,
    /// Path to the original working repository (used by external stage scripts).
    pub repo_root: PathBuf,
    /// URL of the upstream remote.
    pub upstream_url: String,
}

impl ExecEnvironment {
    /// Read the execution environment from AIRLOCK_* environment variables.
    ///
    /// Returns an error if any required environment variable is missing.
    pub fn from_env() -> Result<Self> {
        let run_id = env::var("AIRLOCK_RUN_ID")
            .context("AIRLOCK_RUN_ID not set. This command must be run from within a pipeline.")?;

        let branch = env::var("AIRLOCK_BRANCH")
            .context("AIRLOCK_BRANCH not set. This command must be run from within a pipeline.")?;

        let base_sha = env::var("AIRLOCK_BASE_SHA").context(
            "AIRLOCK_BASE_SHA not set. This command must be run from within a pipeline.",
        )?;

        let head_sha = env::var("AIRLOCK_HEAD_SHA").context(
            "AIRLOCK_HEAD_SHA not set. This command must be run from within a pipeline.",
        )?;

        let worktree = env::var("AIRLOCK_WORKTREE")
            .context("AIRLOCK_WORKTREE not set. This command must be run from within a pipeline.")
            .map(PathBuf::from)?;

        let artifacts = env::var("AIRLOCK_ARTIFACTS")
            .context("AIRLOCK_ARTIFACTS not set. This command must be run from within a pipeline.")
            .map(PathBuf::from)?;

        let repo_root = env::var("AIRLOCK_REPO_ROOT")
            .context("AIRLOCK_REPO_ROOT not set. This command must be run from within a pipeline.")
            .map(PathBuf::from)?;

        let upstream_url = env::var("AIRLOCK_UPSTREAM_URL").context(
            "AIRLOCK_UPSTREAM_URL not set. This command must be run from within a pipeline.",
        )?;

        Ok(Self {
            run_id,
            branch,
            base_sha,
            head_sha,
            worktree,
            artifacts,
            repo_root,
            upstream_url,
        })
    }
}
