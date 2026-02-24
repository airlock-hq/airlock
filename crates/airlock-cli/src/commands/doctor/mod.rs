//! `airlock doctor` command implementation.
//!
//! Diagnoses common issues with Airlock configuration including:
//! - Daemon status
//! - Remote configuration
//! - Hook installation
//! - Database integrity

mod checks;

use anyhow::{Context, Result};
use std::env;
use std::path::Path;
use tracing::info;

use airlock_core::AirlockPaths;

use checks::{
    check_daemon, check_database, check_gate_repo, check_hooks, check_remotes,
    check_repo_enrollment, get_enrolled_repo,
};

/// Represents the result of a single diagnostic check.
#[derive(Debug, Clone)]
pub struct DiagnosticResult {
    /// Name of the check.
    pub name: String,
    /// Whether the check passed.
    pub passed: bool,
    /// Human-readable message describing the result.
    pub message: String,
    /// Optional suggestion for fixing the issue.
    pub suggestion: Option<String>,
}

impl DiagnosticResult {
    pub(crate) fn pass(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed: true,
            message: message.into(),
            suggestion: None,
        }
    }

    pub(crate) fn fail(
        name: impl Into<String>,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            passed: false,
            message: message.into(),
            suggestion: Some(suggestion.into()),
        }
    }

    pub(crate) fn warn(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed: true, // Warnings don't fail the check
            message: message.into(),
            suggestion: None,
        }
    }
}

/// Run the doctor command to diagnose common issues.
pub async fn run() -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let paths = AirlockPaths::new().context("Failed to initialize Airlock paths")?;

    run_with_paths(&current_dir, &paths)
}

/// Internal implementation that accepts paths for testability.
fn run_with_paths(working_dir: &Path, paths: &AirlockPaths) -> Result<()> {
    info!("Running Airlock diagnostics...");

    println!("Airlock Doctor");
    println!("══════════════");
    println!();

    let mut results = Vec::new();

    // Check daemon status
    let daemon_result = check_daemon(paths);
    results.push(daemon_result);

    // Check database integrity
    let db_result = check_database(paths);
    results.push(db_result);

    // Check if we're in a git repo enrolled with Airlock
    let repo_check = check_repo_enrollment(working_dir, paths);
    results.push(repo_check.clone());

    // If repo is enrolled, check remotes and hooks
    if repo_check.passed {
        if let Ok(Some(repo)) = get_enrolled_repo(working_dir, paths) {
            let remote_result = check_remotes(working_dir, &repo);
            results.push(remote_result);

            let hooks_result = check_hooks(&repo.gate_path);
            results.push(hooks_result);

            let gate_result = check_gate_repo(&repo.gate_path);
            results.push(gate_result);
        }
    }

    // Print results
    let mut has_issues = false;
    let mut suggestions = Vec::new();

    for result in &results {
        let status = if result.passed { "✓" } else { "✗" };
        let status_color = if result.passed { "OK" } else { "FAIL" };

        println!("{} {} ... {}", status, result.name, status_color);

        if !result.passed {
            println!("  └─ {}", result.message);
            has_issues = true;
            if let Some(ref suggestion) = result.suggestion {
                suggestions.push((result.name.clone(), suggestion.clone()));
            }
        }
    }

    println!();

    // Print summary
    let passed = results.iter().filter(|r| r.passed).count();
    let total = results.len();

    if has_issues {
        println!("Results: {}/{} checks passed", passed, total);
        println!();
        println!("Suggested Fixes");
        println!("───────────────");
        for (name, suggestion) in &suggestions {
            println!();
            println!("{}:", name);
            println!("  {}", suggestion);
        }
    } else {
        println!("All {} checks passed! Airlock is healthy.", total);
    }

    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
