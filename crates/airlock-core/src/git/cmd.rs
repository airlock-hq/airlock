//! Git command execution helpers.
//!
//! Provides convenience wrappers around `std::process::Command` for running
//! git commands with consistent error handling.

use crate::error::{AirlockError, Result};
use std::path::Path;
use std::process::{Command, Output};

/// Check that a git command output indicates success.
fn check_success(output: Output, operation: &str) -> Result<Output> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AirlockError::Git(format!(
            "git {} failed: {}",
            operation,
            stderr.trim()
        )));
    }
    Ok(output)
}

/// Run a git command in the given repository and check for success.
///
/// Sets `-C <repo_path>` and returns an error if the exit status is non-zero.
pub(crate) fn run_git(repo_path: &Path, args: &[&str], operation: &str) -> Result<Output> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .output()
        .map_err(|e| AirlockError::Git(format!("Failed to execute git {}: {}", operation, e)))?;
    check_success(output, operation)
}

/// Like [`run_git`], but sets `GIT_TERMINAL_PROMPT=0` to disable interactive prompts.
///
/// Use for network operations (fetch, push) to prevent hanging on auth prompts.
pub(crate) fn run_git_network(repo_path: &Path, args: &[&str], operation: &str) -> Result<Output> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| AirlockError::Git(format!("Failed to execute git {}: {}", operation, e)))?;
    check_success(output, operation)
}

/// Run a git command without checking the exit status.
///
/// Use when non-zero exit has meaning rather than indicating failure
/// (e.g., ref not found, commit is not an ancestor).
pub(crate) fn run_git_unchecked(
    repo_path: &Path,
    args: &[&str],
    operation: &str,
) -> Result<Output> {
    Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .output()
        .map_err(|e| AirlockError::Git(format!("Failed to execute git {}: {}", operation, e)))
}
