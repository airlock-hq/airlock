//! `airlock runs` command implementation.

use anyhow::{Context, Result};
use std::env;
use std::path::Path;
use tracing::{debug, info};

use airlock_core::{git, AirlockPaths, Database};

use super::format::{format_status, format_time_ago};

/// Arguments for the runs command.
#[derive(Debug)]
pub struct RunsArgs {
    /// Maximum number of runs to display.
    pub limit: u32,
}

impl Default for RunsArgs {
    fn default() -> Self {
        Self { limit: 20 }
    }
}

/// Run the runs command to list recent runs.
pub async fn run(args: RunsArgs) -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let paths = AirlockPaths::new().context("Failed to initialize Airlock paths")?;

    run_with_paths(&current_dir, &paths, &args)
}

/// Internal implementation that accepts paths for testability.
fn run_with_paths(working_dir: &Path, paths: &AirlockPaths, args: &RunsArgs) -> Result<()> {
    info!("Listing recent runs...");

    // 1. Detect current repo
    let working_repo = git::discover_repo(working_dir).context("Not inside a Git repository")?;

    let working_path = working_repo
        .workdir()
        .ok_or_else(|| anyhow::anyhow!("Cannot list runs in a bare repository"))?
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

    // 3. Get runs
    let runs = db
        .list_runs(&repo.id, Some(args.limit))
        .context("Failed to query runs")?;

    // 4. Format output
    println!("Recent Runs");
    println!("═══════════");
    println!();

    if runs.is_empty() {
        println!("  No runs found.");
        println!();
        println!("  Push to this repository to create a run:");
        println!("    git push origin <branch>");
        return Ok(());
    }

    // Print header
    println!("{:<14} {:<18} {:<20} REFS", "RUN ID", "STATUS", "CREATED");
    println!("{}", "─".repeat(70));

    for run in &runs {
        let derived = db
            .compute_run_status(run)
            .unwrap_or_else(|_| "unknown".to_string());
        let status_str = format_status(&derived);
        let created_str = format_time_ago(run.created_at);
        let refs_str = format_refs(&run.ref_updates);

        // Truncate run ID for display (first 12 chars)
        let run_id_short = if run.id.len() > 12 {
            &run.id[..12]
        } else {
            &run.id
        };

        println!(
            "{:<14} {:<18} {:<20} {}",
            run_id_short, status_str, created_str, refs_str
        );
    }

    println!();
    println!("Showing {} of {} runs", runs.len(), runs.len());
    println!();
    println!("Use 'airlock show <run-id>' for run details");

    Ok(())
}

/// Format ref updates as a short string.
fn format_refs(ref_updates: &[airlock_core::RefUpdate]) -> String {
    if ref_updates.is_empty() {
        return "(none)".to_string();
    }

    if ref_updates.len() == 1 {
        // Show the branch name (strip refs/heads/ prefix)
        let ref_name = &ref_updates[0].ref_name;
        let short_name = ref_name.strip_prefix("refs/heads/").unwrap_or(ref_name);
        return short_name.to_string();
    }

    // Multiple refs
    format!("{} refs", ref_updates.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use airlock_core::{JobResult, JobStatus, RefUpdate, Repo, Run, StepResult, StepStatus};
    use git2::Repository;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::TempDir;

    fn create_test_working_repo(dir: &Path) -> Repository {
        let repo = Repository::init(dir).expect("Failed to init repo");

        // Create an initial commit
        {
            let sig = repo
                .signature()
                .unwrap_or_else(|_| git2::Signature::now("Test", "test@example.com").unwrap());

            let tree_id = {
                let mut index = repo.index().unwrap();
                index.write_tree().unwrap()
            };
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
                .unwrap();
        }

        repo
    }

    fn now_timestamp() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    #[test]
    fn test_format_status() {
        assert_eq!(format_status("running"), "● running");
        assert_eq!(format_status("pending"), "○ pending");
        assert_eq!(format_status("awaiting_approval"), "◐ awaiting");
        assert_eq!(format_status("completed"), "✓ completed");
        assert_eq!(format_status("failed"), "✗ failed");
    }

    #[test]
    fn test_format_refs() {
        // Empty
        let empty: Vec<RefUpdate> = vec![];
        assert_eq!(format_refs(&empty), "(none)");

        // Single ref
        let single = vec![RefUpdate {
            ref_name: "refs/heads/main".to_string(),
            old_sha: "abc".to_string(),
            new_sha: "def".to_string(),
        }];
        assert_eq!(format_refs(&single), "main");

        // Multiple refs
        let multiple = vec![
            RefUpdate {
                ref_name: "refs/heads/main".to_string(),
                old_sha: "abc".to_string(),
                new_sha: "def".to_string(),
            },
            RefUpdate {
                ref_name: "refs/heads/feature".to_string(),
                old_sha: "abc".to_string(),
                new_sha: "def".to_string(),
            },
        ];
        assert_eq!(format_refs(&multiple), "2 refs");
    }

    #[test]
    fn test_runs_fails_outside_git_repo() {
        let temp_dir = TempDir::new().unwrap();
        let airlock_root = temp_dir.path().join("airlock");
        let paths = AirlockPaths::with_root(airlock_root);

        let args = RunsArgs::default();
        let result = run_with_paths(temp_dir.path(), &paths, &args);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Not inside a Git repository"));
    }

    #[test]
    fn test_runs_fails_if_not_enrolled() {
        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().join("working");
        let airlock_root = temp_dir.path().join("airlock");

        fs::create_dir_all(&working_dir).unwrap();
        create_test_working_repo(&working_dir);

        let paths = AirlockPaths::with_root(airlock_root);
        paths.ensure_dirs().unwrap();

        let _db = Database::open(&paths.database()).unwrap();

        let args = RunsArgs::default();
        let result = run_with_paths(&working_dir, &paths, &args);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not enrolled in Airlock"));
    }

    #[test]
    fn test_runs_lists_runs() {
        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().join("working");
        let airlock_root = temp_dir.path().join("airlock");

        fs::create_dir_all(&working_dir).unwrap();
        let repo = create_test_working_repo(&working_dir);
        repo.remote("origin", "https://github.com/user/repo.git")
            .unwrap();

        let paths = AirlockPaths::with_root(airlock_root);
        paths.ensure_dirs().unwrap();

        let db = Database::open(&paths.database()).unwrap();
        let canonical_path = working_dir.canonicalize().unwrap();

        // Create repo
        let test_repo = Repo {
            id: "test123".to_string(),
            working_path: canonical_path.clone(),
            upstream_url: "https://github.com/user/repo.git".to_string(),
            gate_path: PathBuf::from("/tmp/gate.git"),
            last_sync: Some(now_timestamp()),
            created_at: now_timestamp(),
        };
        db.insert_repo(&test_repo).unwrap();

        // Create some runs with stage results to give them derived status
        for i in 0..5 {
            let created = now_timestamp() - (i as i64 * 3600);
            let run = Run {
                id: format!("run{}", i),
                repo_id: "test123".to_string(),
                ref_updates: vec![RefUpdate {
                    ref_name: "refs/heads/main".to_string(),
                    old_sha: "abc".to_string(),
                    new_sha: "def".to_string(),
                }],
                error: None,
                superseded: false,
                created_at: created,
                branch: String::new(),
                base_sha: String::new(),
                head_sha: String::new(),
                current_step: None,
                workflow_file: String::new(),
                workflow_name: None,
                updated_at: created,
            };
            db.insert_run(&run).unwrap();

            // Create job result first (FK constraint)
            let job_status = if i % 2 == 0 {
                JobStatus::Passed
            } else {
                JobStatus::Running
            };
            db.insert_job_result(&JobResult {
                id: format!("job{}", i),
                run_id: format!("run{}", i),
                job_key: "default".to_string(),
                name: Some("default".to_string()),
                status: job_status,
                job_order: 0,
                started_at: None,
                completed_at: None,
                error: None,
                worktree_path: None,
            })
            .unwrap();

            // Give each run a stage result so status is derived
            let stage = StepResult {
                id: format!("sr{}", i),
                run_id: format!("run{}", i),
                job_id: format!("job{}", i),
                name: "test".to_string(),
                status: if i % 2 == 0 {
                    StepStatus::Passed
                } else {
                    StepStatus::Running
                },
                step_order: 0,
                exit_code: None,
                duration_ms: None,
                error: None,
                started_at: None,
                completed_at: None,
            };
            db.insert_step_result(&stage).unwrap();
        }

        // Run with limit
        let args = RunsArgs { limit: 3 };
        let result = run_with_paths(&working_dir, &paths, &args);
        assert!(result.is_ok(), "runs command failed: {:?}", result.err());
    }

    #[test]
    fn test_runs_empty_repo() {
        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().join("working");
        let airlock_root = temp_dir.path().join("airlock");

        fs::create_dir_all(&working_dir).unwrap();
        let repo = create_test_working_repo(&working_dir);
        repo.remote("origin", "https://github.com/user/repo.git")
            .unwrap();

        let paths = AirlockPaths::with_root(airlock_root);
        paths.ensure_dirs().unwrap();

        let db = Database::open(&paths.database()).unwrap();
        let canonical_path = working_dir.canonicalize().unwrap();

        // Create repo without any runs
        let test_repo = Repo {
            id: "test123".to_string(),
            working_path: canonical_path.clone(),
            upstream_url: "https://github.com/user/repo.git".to_string(),
            gate_path: PathBuf::from("/tmp/gate.git"),
            last_sync: Some(now_timestamp()),
            created_at: now_timestamp(),
        };
        db.insert_repo(&test_repo).unwrap();

        let args = RunsArgs::default();
        let result = run_with_paths(&working_dir, &paths, &args);
        assert!(result.is_ok(), "runs command failed: {:?}", result.err());
    }
}
