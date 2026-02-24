//! `airlock show` command implementation.

use anyhow::{Context, Result};
use tracing::{debug, info};

use airlock_core::{AirlockPaths, Database, StepStatus};

use super::format::format_time_ago;
use super::lookup::find_run_by_prefix;

/// Arguments for the show command.
#[derive(Debug)]
pub struct ShowArgs {
    /// The run ID to show details for.
    pub run_id: String,
}

/// Run the show command to display run details.
pub async fn run(args: ShowArgs) -> Result<()> {
    let paths = AirlockPaths::new().context("Failed to initialize Airlock paths")?;

    run_with_paths(&paths, &args)
}

/// Internal implementation that accepts paths for testability.
fn run_with_paths(paths: &AirlockPaths, args: &ShowArgs) -> Result<()> {
    info!("Showing run details...");

    // 1. Open database
    let db = Database::open(&paths.database()).context("Failed to open Airlock database")?;

    // 2. Look up run by ID (support prefix matching for convenience)
    let run = find_run_by_prefix(&db, &args.run_id)?;

    debug!("Found run: {}", run.id);

    // 3. Get the repo for context
    let repo = db
        .get_repo(&run.repo_id)
        .context("Failed to query repo")?
        .ok_or_else(|| anyhow::anyhow!("Repository not found for this run"))?;

    // 4. Get stage results for this run
    let stages = db
        .get_step_results_for_run(&run.id)
        .context("Failed to query stage results")?;

    // 5. Get derived status
    let status = run.derived_status(&stages);

    // 6. Format output
    println!("Run Details");
    println!("═══════════");
    println!();

    // Run info
    println!("Run ID:     {}", run.id);
    println!("Status:     {}", format_derived_status(status));
    println!(
        "Branch:     {}",
        if run.branch.is_empty() {
            "(unknown)"
        } else {
            &run.branch
        }
    );
    println!("Repository: {}", repo.working_path.display());
    println!();

    // Ref updates
    println!("Ref Updates");
    println!("───────────");
    if run.ref_updates.is_empty() {
        println!("  (none)");
    } else {
        for ref_update in &run.ref_updates {
            let short_name = ref_update
                .ref_name
                .strip_prefix("refs/heads/")
                .unwrap_or(&ref_update.ref_name);
            let old_short = if ref_update.old_sha.len() > 7 {
                &ref_update.old_sha[..7]
            } else {
                &ref_update.old_sha
            };
            let new_short = if ref_update.new_sha.len() > 7 {
                &ref_update.new_sha[..7]
            } else {
                &ref_update.new_sha
            };

            // Check if it's a new branch (all zeros)
            if ref_update.old_sha == "0000000000000000000000000000000000000000" {
                println!("  {} (new) → {}", short_name, new_short);
            } else if ref_update.new_sha == "0000000000000000000000000000000000000000" {
                println!("  {} {} → (deleted)", short_name, old_short);
            } else {
                println!("  {} {}..{}", short_name, old_short, new_short);
            }
        }
    }
    println!();

    // Timestamps
    println!("Timestamps");
    println!("──────────");
    println!(
        "  Created:   {} ({})",
        format_timestamp(run.created_at),
        format_time_ago(run.created_at)
    );
    // Compute completed_at from stage results when run is completed
    if status == "completed" || status == "failed" {
        if let Some(max_completed) = stages.iter().filter_map(|s| s.completed_at).max() {
            println!(
                "  Completed: {} ({})",
                format_timestamp(max_completed),
                format_time_ago(max_completed)
            );
            let duration = max_completed - run.created_at;
            println!("  Duration:  {}", format_duration(duration));
        }
    }
    println!();

    // Error (if failed)
    if let Some(ref error) = run.error {
        println!("Error");
        println!("─────");
        println!("  {}", error);
        println!();
    }

    // Pipeline Stages
    println!("Pipeline Stages");
    println!("───────────────");
    if stages.is_empty() {
        println!("  No stages yet (pipeline may still be starting)");
    } else {
        println!("  {:<15} {:<18} {:<10}", "STAGE", "STATUS", "DURATION");
        println!("  {}", "─".repeat(50));

        for stage in &stages {
            let status_str = format_stage_status(stage.status);
            let duration_str = stage
                .duration_ms
                .map(|ms| format!("{}ms", ms))
                .unwrap_or_else(|| "-".to_string());

            println!(
                "  {:<15} {:<18} {:<10}",
                stage.name, status_str, duration_str
            );

            // Show error if failed
            if let Some(ref error) = stage.error {
                println!("    └─ Error: {}", error);
            }
        }
    }
    println!();

    // Actions hint based on derived status
    match status {
        "awaiting_approval" => {
            println!("Actions");
            println!("───────");
            println!("  This run is awaiting approval. Use the desktop app to approve or reject.");
        }
        "running" | "pending" => {
            println!("Actions");
            println!("───────");
            println!("  This run is still in progress. Check back later.");
        }
        _ => {}
    }

    Ok(())
}

/// Format derived run status with indicator.
fn format_derived_status(status: &str) -> String {
    match status {
        "running" => "● Running".to_string(),
        "pending" => "○ Pending".to_string(),
        "awaiting_approval" => "◐ Awaiting Approval".to_string(),
        "completed" => "✓ Completed".to_string(),
        "failed" => "✗ Failed".to_string(),
        _ => format!("? {}", status),
    }
}

/// Format stage status with indicator.
fn format_stage_status(status: StepStatus) -> String {
    match status {
        StepStatus::Pending => "○ pending".to_string(),
        StepStatus::Running => "● running".to_string(),
        StepStatus::Passed => "✓ passed".to_string(),
        StepStatus::Failed => "✗ failed".to_string(),
        StepStatus::Skipped => "- skipped".to_string(),
        StepStatus::AwaitingApproval => "◐ awaiting approval".to_string(),
    }
}

/// Format a Unix timestamp as a human-readable date/time.
fn format_timestamp(timestamp: i64) -> String {
    // Simple ISO-like format
    // In production, we'd use chrono for proper formatting
    let secs = timestamp;
    let days_since_epoch = secs / 86400;

    // Approximate year calculation
    let years = days_since_epoch / 365;
    let year = 1970 + years;

    // Remaining days in year
    let day_of_year = days_since_epoch % 365;

    // Approximate month and day
    let month = (day_of_year / 30) + 1;
    let day = (day_of_year % 30) + 1;

    // Time of day
    let secs_of_day = secs % 86400;
    let hours = secs_of_day / 3600;
    let minutes = (secs_of_day % 3600) / 60;

    format!(
        "{}-{:02}-{:02} {:02}:{:02}",
        year, month, day, hours, minutes
    )
}

/// Format a duration in seconds as a human-readable string.
fn format_duration(seconds: i64) -> String {
    if seconds < 0 {
        return "invalid".to_string();
    }

    let seconds = seconds as u64;

    if seconds < 60 {
        return format!("{}s", seconds);
    }

    let minutes = seconds / 60;
    let secs = seconds % 60;

    if minutes < 60 {
        return format!("{}m {}s", minutes, secs);
    }

    let hours = minutes / 60;
    let mins = minutes % 60;

    format!("{}h {}m", hours, mins)
}

#[cfg(test)]
mod tests {
    use super::*;
    use airlock_core::{JobResult, JobStatus, RefUpdate, Repo, Run, StepResult};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::TempDir;

    fn now_timestamp() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    #[test]
    fn test_format_derived_status() {
        assert_eq!(format_derived_status("running"), "● Running");
        assert_eq!(format_derived_status("pending"), "○ Pending");
        assert_eq!(
            format_derived_status("awaiting_approval"),
            "◐ Awaiting Approval"
        );
        assert_eq!(format_derived_status("completed"), "✓ Completed");
        assert_eq!(format_derived_status("failed"), "✗ Failed");
    }

    #[test]
    fn test_format_stage_status() {
        assert_eq!(format_stage_status(StepStatus::Pending), "○ pending");
        assert_eq!(format_stage_status(StepStatus::Running), "● running");
        assert_eq!(format_stage_status(StepStatus::Passed), "✓ passed");
        assert_eq!(format_stage_status(StepStatus::Failed), "✗ failed");
        assert_eq!(format_stage_status(StepStatus::Skipped), "- skipped");
        assert_eq!(
            format_stage_status(StepStatus::AwaitingApproval),
            "◐ awaiting approval"
        );
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3661), "1h 1m");
    }

    #[test]
    fn test_show_run_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let airlock_root = temp_dir.path().join("airlock");
        let paths = AirlockPaths::with_root(airlock_root);
        paths.ensure_dirs().unwrap();

        let _db = Database::open(&paths.database()).unwrap();

        let args = ShowArgs {
            run_id: "nonexistent".to_string(),
        };
        let result = run_with_paths(&paths, &args);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No run found"));
    }

    #[test]
    fn test_show_run_details() {
        let temp_dir = TempDir::new().unwrap();
        let airlock_root = temp_dir.path().join("airlock");
        let paths = AirlockPaths::with_root(airlock_root);
        paths.ensure_dirs().unwrap();

        let db = Database::open(&paths.database()).unwrap();

        // Create repo
        let test_repo = Repo {
            id: "test123".to_string(),
            working_path: PathBuf::from("/tmp/test"),
            upstream_url: "https://github.com/user/repo.git".to_string(),
            gate_path: PathBuf::from("/tmp/gate.git"),
            last_sync: Some(now_timestamp()),
            created_at: now_timestamp(),
        };
        db.insert_repo(&test_repo).unwrap();

        // Create run
        let created = now_timestamp() - 300;
        let run = Run {
            id: "run123456789".to_string(),
            repo_id: "test123".to_string(),
            ref_updates: vec![RefUpdate {
                ref_name: "refs/heads/main".to_string(),
                old_sha: "abc1234567890".to_string(),
                new_sha: "def1234567890".to_string(),
            }],
            error: None,
            superseded: false,
            created_at: created,
            branch: "main".to_string(),
            base_sha: "abc1234567890".to_string(),
            head_sha: "def1234567890".to_string(),
            current_step: Some("review".to_string()),
            updated_at: created,
            workflow_file: String::new(),
            workflow_name: None,
        };
        db.insert_run(&run).unwrap();

        // Create job result first (FK constraint)
        db.insert_job_result(&JobResult {
            id: "job_show".to_string(),
            run_id: "run123456789".to_string(),
            job_key: "default".to_string(),
            name: Some("default".to_string()),
            status: JobStatus::Running,
            job_order: 0,
            started_at: None,
            completed_at: None,
            error: None,
        })
        .unwrap();

        // Create stage results
        let stage1 = StepResult {
            id: "stage1".to_string(),
            run_id: "run123456789".to_string(),
            job_id: "job_show".to_string(),
            name: "describe".to_string(),
            status: StepStatus::Passed,
            step_order: 0,
            exit_code: Some(0),
            duration_ms: Some(1234),
            error: None,
            started_at: Some(created),
            completed_at: Some(created + 1),
        };
        db.insert_step_result(&stage1).unwrap();

        let stage2 = StepResult {
            id: "stage2".to_string(),
            run_id: "run123456789".to_string(),
            job_id: "job_show".to_string(),
            name: "review".to_string(),
            status: StepStatus::AwaitingApproval,
            step_order: 1,
            exit_code: None,
            duration_ms: None,
            error: None,
            started_at: Some(created + 2),
            completed_at: None,
        };
        db.insert_step_result(&stage2).unwrap();

        // Show by exact ID
        let args = ShowArgs {
            run_id: "run123456789".to_string(),
        };
        let result = run_with_paths(&paths, &args);
        assert!(result.is_ok(), "show command failed: {:?}", result.err());
    }

    #[test]
    fn test_show_run_by_prefix() {
        let temp_dir = TempDir::new().unwrap();
        let airlock_root = temp_dir.path().join("airlock");
        let paths = AirlockPaths::with_root(airlock_root);
        paths.ensure_dirs().unwrap();

        let db = Database::open(&paths.database()).unwrap();

        // Create repo
        let test_repo = Repo {
            id: "test123".to_string(),
            working_path: PathBuf::from("/tmp/test"),
            upstream_url: "https://github.com/user/repo.git".to_string(),
            gate_path: PathBuf::from("/tmp/gate.git"),
            last_sync: Some(now_timestamp()),
            created_at: now_timestamp(),
        };
        db.insert_repo(&test_repo).unwrap();

        // Create run
        let created = now_timestamp();
        let run = Run {
            id: "run_unique_id_12345".to_string(),
            repo_id: "test123".to_string(),
            ref_updates: vec![],
            error: None,
            superseded: false,
            created_at: created,
            branch: String::new(),
            base_sha: String::new(),
            head_sha: String::new(),
            current_step: None,
            updated_at: created,
            workflow_file: String::new(),
            workflow_name: None,
        };
        db.insert_run(&run).unwrap();

        // Show by prefix
        let args = ShowArgs {
            run_id: "run_unique".to_string(),
        };
        let result = run_with_paths(&paths, &args);
        assert!(result.is_ok(), "show by prefix failed: {:?}", result.err());
    }

    #[test]
    fn test_show_ambiguous_prefix() {
        let temp_dir = TempDir::new().unwrap();
        let airlock_root = temp_dir.path().join("airlock");
        let paths = AirlockPaths::with_root(airlock_root);
        paths.ensure_dirs().unwrap();

        let db = Database::open(&paths.database()).unwrap();

        // Create repo
        let test_repo = Repo {
            id: "test123".to_string(),
            working_path: PathBuf::from("/tmp/test"),
            upstream_url: "https://github.com/user/repo.git".to_string(),
            gate_path: PathBuf::from("/tmp/gate.git"),
            last_sync: Some(now_timestamp()),
            created_at: now_timestamp(),
        };
        db.insert_repo(&test_repo).unwrap();

        // Create two runs with similar IDs
        let created = now_timestamp();
        let run1 = Run {
            id: "run_abc_123".to_string(),
            repo_id: "test123".to_string(),
            ref_updates: vec![],
            error: None,
            superseded: false,
            created_at: created,
            branch: String::new(),
            base_sha: String::new(),
            head_sha: String::new(),
            current_step: None,
            updated_at: created,
            workflow_file: String::new(),
            workflow_name: None,
        };
        db.insert_run(&run1).unwrap();

        let run2 = Run {
            id: "run_abc_456".to_string(),
            repo_id: "test123".to_string(),
            ref_updates: vec![],
            error: None,
            superseded: false,
            created_at: created,
            branch: String::new(),
            base_sha: String::new(),
            head_sha: String::new(),
            current_step: None,
            updated_at: created,
            workflow_file: String::new(),
            workflow_name: None,
        };
        db.insert_run(&run2).unwrap();

        // Try ambiguous prefix
        let args = ShowArgs {
            run_id: "run_abc".to_string(),
        };
        let result = run_with_paths(&paths, &args);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Ambiguous"));
    }
}
