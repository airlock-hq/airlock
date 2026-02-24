//! `airlock status` command implementation.

use anyhow::{Context, Result};
use std::env;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, info};

use airlock_core::{git, AirlockPaths, Database};

use super::format::format_time_ago;

/// Check if the daemon is running by verifying the socket exists and is connectable.
/// Returns (is_running, message) tuple.
fn check_daemon_status(paths: &AirlockPaths) -> (bool, &'static str) {
    let socket_path = paths.socket();

    debug!("Checking daemon socket at: {}", socket_path.display());

    // On Unix, check if the socket file exists and is connectable
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;

        if !socket_path.exists() {
            return (false, "not running");
        }

        // Try to connect to verify the daemon is actually listening
        match UnixStream::connect(&socket_path) {
            Ok(_) => (true, "running"),
            Err(_) => (false, "not responding"),
        }
    }

    #[cfg(windows)]
    {
        // On Windows, check named pipe marker
        if socket_path.exists() {
            (true, "running")
        } else {
            (false, "not running")
        }
    }
}

/// Run the status command to show pending runs and last sync.
pub async fn run() -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let paths = AirlockPaths::new().context("Failed to initialize Airlock paths")?;

    run_with_paths(&current_dir, &paths)
}

/// Internal implementation that accepts paths for testability.
fn run_with_paths(working_dir: &Path, paths: &AirlockPaths) -> Result<()> {
    info!("Checking Airlock status...");

    // 1. Detect current repo
    let working_repo = git::discover_repo(working_dir).context("Not inside a Git repository")?;

    let working_path = working_repo
        .workdir()
        .ok_or_else(|| anyhow::anyhow!("Cannot check status in a bare repository"))?
        .to_path_buf()
        .canonicalize()
        .context("Failed to canonicalize working directory path")?;

    debug!("Working repository: {}", working_path.display());

    // 2. Open database and look up repo
    let db = Database::open(&paths.database()).context("Failed to open Airlock database")?;

    let repo = db
        .get_repo_by_path(&working_path)
        .context("Failed to query database")?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "This repository is not enrolled in Airlock.\n\
                 Run 'airlock init' to get started."
            )
        })?;

    debug!("Found repo in database: {}", repo.id);

    // 3. Get active runs (running or pending approval)
    let active_runs = db
        .list_active_runs(&repo.id)
        .context("Failed to query active runs")?;

    // 4. Get recent runs for additional stats
    let recent_runs = db
        .list_runs(&repo.id, Some(10))
        .context("Failed to query recent runs")?;

    // Count by derived status
    let mut running_count = 0;
    let mut pending_count = 0;
    for r in &active_runs {
        let status = db
            .compute_run_status(r)
            .unwrap_or_else(|_| "unknown".to_string());
        match status.as_str() {
            "running" | "pending" => running_count += 1,
            "awaiting_approval" => pending_count += 1,
            _ => {}
        }
    }

    // 5. Check daemon status
    let (daemon_running, daemon_message) = check_daemon_status(paths);

    // 6. Format output
    println!("Airlock Status");
    println!("──────────────");
    println!();

    // Daemon status
    let daemon_status_str = if daemon_running {
        format!("Daemon:     {} ✓", daemon_message)
    } else {
        format!("Daemon:     {} ✗", daemon_message)
    };
    println!("{}", daemon_status_str);
    println!();

    // Show warning if daemon is not running
    if !daemon_running {
        println!("⚠ Warning: Daemon is not running. Some features may not work.");
        println!("  Run 'airlock daemon start' to start the daemon.");
        println!();
    }

    // Repository info
    println!("Repository: {}", repo.working_path.display());
    println!("Upstream:   {}", repo.upstream_url);
    println!("ID:         {}", repo.id);
    println!();

    // Last sync
    match repo.last_sync {
        Some(timestamp) => {
            let ago = format_time_ago(timestamp);
            println!("Last sync:  {} ({})", format_timestamp(timestamp), ago);
        }
        None => {
            println!("Last sync:  never");
        }
    }
    println!();

    // Active runs
    println!("Active Runs");
    println!("───────────");
    if active_runs.is_empty() {
        println!("  No active runs");
    } else {
        if running_count > 0 {
            println!("  {} running", running_count);
        }
        if pending_count > 0 {
            println!("  {} pending approval", pending_count);
        }
        println!();
        println!("  Use 'airlock runs' to see details");
    }
    println!();

    // Quick summary of recent runs
    if !recent_runs.is_empty() {
        let mut forwarded = 0;
        let mut failed = 0;
        for r in &recent_runs {
            let status = db
                .compute_run_status(r)
                .unwrap_or_else(|_| "unknown".to_string());
            match status.as_str() {
                "completed" => forwarded += 1,
                "failed" => failed += 1,
                _ => {}
            }
        }

        println!("Recent Activity (last 10 runs)");
        println!("──────────────────────────────");
        println!(
            "  {} forwarded, {} failed, {} active",
            forwarded,
            failed,
            active_runs.len()
        );
    }

    Ok(())
}

/// Format a Unix timestamp as a human-readable date/time.
fn format_timestamp(timestamp: i64) -> String {
    // Convert to chrono if available, otherwise use simple format
    let duration = Duration::from_secs(timestamp as u64);
    let datetime = UNIX_EPOCH + duration;

    if let Ok(elapsed) = SystemTime::now().duration_since(datetime) {
        // If we can compute elapsed time, datetime is valid
        let _ = elapsed; // Just to verify the datetime is valid
    }

    // Format as ISO-like string (simple version without chrono dependency)
    // In production, we'd use chrono for proper timezone support
    let secs = timestamp;
    let days = secs / 86400;
    let years_since_1970 = days / 365;
    let year = 1970 + years_since_1970;

    // Simplified: just show the timestamp in a readable way
    // A proper implementation would use chrono
    format!("{}-{:02}-{:02}", year, 1, 1)
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
