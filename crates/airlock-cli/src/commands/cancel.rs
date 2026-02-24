//! `airlock cancel` command implementation.

use anyhow::{Context, Result};
use tracing::info;

use airlock_core::{AirlockPaths, Database};

use super::lookup::find_run_by_prefix;

/// Arguments for the cancel command.
#[derive(Debug)]
pub struct CancelArgs {
    /// The run ID (or prefix) to cancel.
    pub run_id: String,
}

/// Run the cancel command to cancel a stuck or running run.
pub async fn run(args: CancelArgs) -> Result<()> {
    let paths = AirlockPaths::new().context("Failed to initialize Airlock paths")?;

    run_with_paths(&paths, &args)
}

/// Internal implementation that accepts paths for testability.
fn run_with_paths(paths: &AirlockPaths, args: &CancelArgs) -> Result<()> {
    info!("Cancelling run...");

    // 1. Open database
    let db = Database::open(&paths.database()).context("Failed to open Airlock database")?;

    // 2. Look up run by ID (support prefix matching for convenience)
    let run = find_run_by_prefix(&db, &args.run_id)?;

    // 3. Check if run can be cancelled (use derived status from stages)
    let stages = db
        .get_step_results_for_run(&run.id)
        .context("Failed to get stage results")?;
    let status = run.derived_status(&stages);

    match status {
        "running" | "awaiting_approval" | "pending" => {
            // These statuses can be cancelled
        }
        "completed" => {
            anyhow::bail!(
                "Run {} has already completed and cannot be cancelled",
                &run.id[..12.min(run.id.len())]
            );
        }
        "failed" => {
            anyhow::bail!("Run {} has already failed", &run.id[..12.min(run.id.len())]);
        }
        _ => {
            anyhow::bail!(
                "Run {} is in state '{}' and cannot be cancelled",
                &run.id[..12.min(run.id.len())],
                status
            );
        }
    }

    // 4. Update run with cancellation error
    db.update_run_error(&run.id, Some("Cancelled by user"))
        .context("Failed to update run")?;

    println!("✗ Cancelled run {}", &run.id[..12.min(run.id.len())]);
    println!();
    println!("You can push again to create a new run.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use airlock_core::{JobResult, JobStatus, RefUpdate, Repo, Run, StepResult, StepStatus};
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
    fn test_cancel_running_run() {
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

        // Create run with a running stage (so derived status = "running")
        let created = now_timestamp();
        let run = Run {
            id: "run_to_cancel_123".to_string(),
            repo_id: "test123".to_string(),
            ref_updates: vec![RefUpdate {
                ref_name: "refs/heads/main".to_string(),
                old_sha: "abc1234567890".to_string(),
                new_sha: "def1234567890".to_string(),
            }],
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
        db.insert_job_result(&JobResult {
            id: "job_cancel".to_string(),
            run_id: "run_to_cancel_123".to_string(),
            job_key: "default".to_string(),
            name: Some("default".to_string()),
            status: JobStatus::Running,
            job_order: 0,
            started_at: None,
            completed_at: None,
            error: None,
        })
        .unwrap();
        db.insert_step_result(&StepResult {
            id: "sr_cancel".to_string(),
            run_id: "run_to_cancel_123".to_string(),
            job_id: "job_cancel".to_string(),
            name: "test".to_string(),
            status: StepStatus::Running,
            step_order: 0,
            exit_code: None,
            duration_ms: None,
            error: None,
            started_at: None,
            completed_at: None,
        })
        .unwrap();

        // Cancel the run
        let args = CancelArgs {
            run_id: "run_to_cancel".to_string(),
        };
        let result = run_with_paths(&paths, &args);
        assert!(result.is_ok(), "cancel command failed: {:?}", result.err());

        // Verify error was set
        let updated_run = db.get_run("run_to_cancel_123").unwrap().unwrap();
        assert_eq!(updated_run.error, Some("Cancelled by user".to_string()));
    }

    #[test]
    fn test_cancel_already_completed_run() {
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

        // Create completed run (all stages passed)
        let created = now_timestamp();
        let run = Run {
            id: "run_completed_123".to_string(),
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
        db.insert_job_result(&JobResult {
            id: "job_done".to_string(),
            run_id: "run_completed_123".to_string(),
            job_key: "default".to_string(),
            name: Some("default".to_string()),
            status: JobStatus::Passed,
            job_order: 0,
            started_at: None,
            completed_at: None,
            error: None,
        })
        .unwrap();
        db.insert_step_result(&StepResult {
            id: "sr_done".to_string(),
            run_id: "run_completed_123".to_string(),
            job_id: "job_done".to_string(),
            name: "test".to_string(),
            status: StepStatus::Passed,
            step_order: 0,
            exit_code: Some(0),
            duration_ms: None,
            error: None,
            started_at: None,
            completed_at: None,
        })
        .unwrap();

        // Try to cancel - should fail
        let args = CancelArgs {
            run_id: "run_completed".to_string(),
        };
        let result = run_with_paths(&paths, &args);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("already completed"));
    }

    #[test]
    fn test_cancel_run_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let airlock_root = temp_dir.path().join("airlock");
        let paths = AirlockPaths::with_root(airlock_root);
        paths.ensure_dirs().unwrap();

        let _db = Database::open(&paths.database()).unwrap();

        let args = CancelArgs {
            run_id: "nonexistent".to_string(),
        };
        let result = run_with_paths(&paths, &args);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No run found"));
    }
}
