//! Tests for database operations.

#[cfg(test)]
mod tests {
    use crate::db::Database;
    use crate::error::AirlockError;
    use crate::types::{
        JobResult, JobStatus, RefUpdate, Repo, Run, StepResult, StepStatus, SyncLog,
    };
    use std::path::PathBuf;

    /// Current database schema version for migrations.
    const SCHEMA_VERSION: i32 = 9;

    fn create_test_repo(id: &str) -> Repo {
        Repo {
            id: id.to_string(),
            working_path: PathBuf::from("/tmp/test-repo"),
            upstream_url: "git@github.com:user/repo.git".to_string(),
            gate_path: PathBuf::from("/tmp/.airlock/repos/test.git"),
            last_sync: None,
            created_at: 1704067200, // 2024-01-01
        }
    }

    fn create_test_run(id: &str, repo_id: &str) -> Run {
        Run {
            id: id.to_string(),
            repo_id: repo_id.to_string(),
            ref_updates: vec![RefUpdate {
                ref_name: "refs/heads/main".to_string(),
                old_sha: "abc123".to_string(),
                new_sha: "def456".to_string(),
            }],
            branch: "refs/heads/main".to_string(),
            base_sha: "abc123".to_string(),
            head_sha: "def456".to_string(),
            current_step: None,
            error: None,
            superseded: false,
            workflow_file: "main.yml".to_string(),
            workflow_name: Some("Main Pipeline".to_string()),
            created_at: 1704067200,
            updated_at: 1704067200,
        }
    }

    fn create_test_job_result(id: &str, run_id: &str, job_key: &str) -> JobResult {
        JobResult {
            id: id.to_string(),
            run_id: run_id.to_string(),
            job_key: job_key.to_string(),
            name: Some(format!("Job {}", job_key)),
            status: JobStatus::Pending,
            job_order: 0,
            started_at: None,
            completed_at: None,
            error: None,
            worktree_path: None,
        }
    }

    fn create_test_step_result(id: &str, run_id: &str, job_id: &str, name: &str) -> StepResult {
        StepResult {
            id: id.to_string(),
            run_id: run_id.to_string(),
            job_id: job_id.to_string(),
            name: name.to_string(),
            status: StepStatus::Pending,
            step_order: 0,
            exit_code: None,
            duration_ms: None,
            error: None,
            started_at: None,
            completed_at: None,
        }
    }

    fn create_test_sync_log(id: &str, repo_id: &str) -> SyncLog {
        SyncLog {
            id: id.to_string(),
            repo_id: repo_id.to_string(),
            success: true,
            error: None,
            synced_at: 1704067200,
        }
    }

    /// Helper to set up a repo + run + job for step result tests.
    fn setup_repo_run_job(db: &Database) -> (String, String, String) {
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();
        let run = create_test_run("run1", "repo1");
        db.insert_run(&run).unwrap();
        let job = create_test_job_result("job1", "run1", "default");
        db.insert_job_result(&job).unwrap();
        ("repo1".to_string(), "run1".to_string(), "job1".to_string())
    }

    #[test]
    fn test_database_initialization() {
        let db = Database::open_in_memory().unwrap();
        let version = db.get_schema_version().unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_schema_version_9() {
        let db = Database::open_in_memory().unwrap();
        let version = db.get_schema_version().unwrap();
        assert_eq!(version, 9);
    }

    #[test]
    fn test_repo_crud() {
        let db = Database::open_in_memory().unwrap();
        let repo = create_test_repo("repo1");

        // Insert
        db.insert_repo(&repo).unwrap();

        // Get by ID
        let fetched = db.get_repo("repo1").unwrap().unwrap();
        assert_eq!(fetched.id, "repo1");
        assert_eq!(fetched.upstream_url, repo.upstream_url);

        // Get by path
        let by_path = db.get_repo_by_path(&repo.working_path).unwrap().unwrap();
        assert_eq!(by_path.id, "repo1");

        // List
        let repos = db.list_repos().unwrap();
        assert_eq!(repos.len(), 1);

        // Update last sync
        db.update_repo_last_sync("repo1", 1704153600).unwrap();
        let updated = db.get_repo("repo1").unwrap().unwrap();
        assert_eq!(updated.last_sync, Some(1704153600));

        // Delete
        db.delete_repo("repo1").unwrap();
        assert!(db.get_repo("repo1").unwrap().is_none());
    }

    #[test]
    fn test_run_crud() {
        let db = Database::open_in_memory().unwrap();
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();

        let run = create_test_run("run1", "repo1");

        // Insert
        db.insert_run(&run).unwrap();

        // Get
        let fetched = db.get_run("run1").unwrap().unwrap();
        assert_eq!(fetched.id, "run1");
        assert_eq!(fetched.branch, "refs/heads/main");
        assert_eq!(fetched.base_sha, "abc123");
        assert_eq!(fetched.head_sha, "def456");
        assert_eq!(fetched.workflow_file, "main.yml");
        assert_eq!(fetched.workflow_name, Some("Main Pipeline".to_string()));

        // List
        let runs = db.list_runs("repo1", None).unwrap();
        assert_eq!(runs.len(), 1);

        // Update current_step
        db.update_run_current_step("run1", Some("test")).unwrap();
        let updated = db.get_run("run1").unwrap().unwrap();
        assert_eq!(updated.current_step, Some("test".to_string()));

        // Update error
        db.update_run_error("run1", Some("Test error")).unwrap();
        let updated = db.get_run("run1").unwrap().unwrap();
        assert_eq!(updated.error, Some("Test error".to_string()));

        // Delete
        db.delete_run("run1").unwrap();
        assert!(db.get_run("run1").unwrap().is_none());
    }

    #[test]
    fn test_run_workflow_fields() {
        let db = Database::open_in_memory().unwrap();
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();

        // Run with workflow fields
        let mut run = create_test_run("run1", "repo1");
        run.workflow_file = "hotfix.yml".to_string();
        run.workflow_name = Some("Hotfix Pipeline".to_string());
        db.insert_run(&run).unwrap();

        let fetched = db.get_run("run1").unwrap().unwrap();
        assert_eq!(fetched.workflow_file, "hotfix.yml");
        assert_eq!(fetched.workflow_name, Some("Hotfix Pipeline".to_string()));

        // Run without workflow name
        let mut run2 = create_test_run("run2", "repo1");
        run2.workflow_file = "ci.yml".to_string();
        run2.workflow_name = None;
        db.insert_run(&run2).unwrap();

        let fetched2 = db.get_run("run2").unwrap().unwrap();
        assert_eq!(fetched2.workflow_file, "ci.yml");
        assert_eq!(fetched2.workflow_name, None);
    }

    // =========================================================================
    // Job Result Tests
    // =========================================================================

    #[test]
    fn test_job_result_crud() {
        let db = Database::open_in_memory().unwrap();
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();
        let run = create_test_run("run1", "repo1");
        db.insert_run(&run).unwrap();

        let job = create_test_job_result("job1", "run1", "lint");

        // Insert
        db.insert_job_result(&job).unwrap();

        // Get
        let fetched = db.get_job_result("job1").unwrap().unwrap();
        assert_eq!(fetched.id, "job1");
        assert_eq!(fetched.job_key, "lint");
        assert_eq!(fetched.name, Some("Job lint".to_string()));
        assert_eq!(fetched.status, JobStatus::Pending);

        // Get all for run
        let results = db.get_job_results_for_run("run1").unwrap();
        assert_eq!(results.len(), 1);

        // Update status
        db.update_job_status("job1", JobStatus::Running, Some(1704067200), None, None)
            .unwrap();
        let fetched = db.get_job_result("job1").unwrap().unwrap();
        assert_eq!(fetched.status, JobStatus::Running);
        assert_eq!(fetched.started_at, Some(1704067200));

        // Complete with passed
        db.update_job_status("job1", JobStatus::Passed, None, Some(1704067210), None)
            .unwrap();
        let fetched = db.get_job_result("job1").unwrap().unwrap();
        assert_eq!(fetched.status, JobStatus::Passed);
        assert_eq!(fetched.started_at, Some(1704067200)); // Preserved from previous update
        assert_eq!(fetched.completed_at, Some(1704067210));

        // Delete all for run
        let deleted = db.delete_job_results_for_run("run1").unwrap();
        assert_eq!(deleted, 1);
        assert!(db.get_job_result("job1").unwrap().is_none());
    }

    #[test]
    fn test_job_result_all_statuses() {
        let db = Database::open_in_memory().unwrap();
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();
        let run = create_test_run("run1", "repo1");
        db.insert_run(&run).unwrap();

        let statuses = [
            JobStatus::Pending,
            JobStatus::Running,
            JobStatus::Passed,
            JobStatus::Failed,
            JobStatus::Skipped,
            JobStatus::AwaitingApproval,
        ];

        for (i, status) in statuses.iter().enumerate() {
            let mut job =
                create_test_job_result(&format!("job{}", i), "run1", &format!("job{}", i));
            job.status = *status;
            db.insert_job_result(&job).unwrap();

            let fetched = db.get_job_result(&format!("job{}", i)).unwrap().unwrap();
            assert_eq!(fetched.status, *status, "Status mismatch for {:?}", status);
        }
    }

    #[test]
    fn test_job_result_with_error() {
        let db = Database::open_in_memory().unwrap();
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();
        let run = create_test_run("run1", "repo1");
        db.insert_run(&run).unwrap();

        let job = create_test_job_result("job1", "run1", "test");
        db.insert_job_result(&job).unwrap();

        // Fail job with error
        db.update_job_status(
            "job1",
            JobStatus::Failed,
            Some(1704067200),
            Some(1704067210),
            Some("Test job failed"),
        )
        .unwrap();

        let fetched = db.get_job_result("job1").unwrap().unwrap();
        assert_eq!(fetched.status, JobStatus::Failed);
        assert_eq!(fetched.error, Some("Test job failed".to_string()));
    }

    #[test]
    fn test_job_results_ordering() {
        let db = Database::open_in_memory().unwrap();
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();
        let run = create_test_run("run1", "repo1");
        db.insert_run(&run).unwrap();

        // Insert jobs in non-order
        let mut job2 = create_test_job_result("job2", "run1", "test");
        job2.job_order = 2;
        let mut job0 = create_test_job_result("job0", "run1", "lint");
        job0.job_order = 0;
        let mut job1 = create_test_job_result("job1", "run1", "build");
        job1.job_order = 1;

        db.insert_job_result(&job2).unwrap();
        db.insert_job_result(&job0).unwrap();
        db.insert_job_result(&job1).unwrap();

        let results = db.get_job_results_for_run("run1").unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].job_key, "lint");
        assert_eq!(results[1].job_key, "build");
        assert_eq!(results[2].job_key, "test");
    }

    // =========================================================================
    // Step Result Tests
    // =========================================================================

    #[test]
    fn test_step_result_crud() {
        let db = Database::open_in_memory().unwrap();
        let (_repo_id, _run_id, job_id) = setup_repo_run_job(&db);

        let step_result = create_test_step_result("sr1", "run1", &job_id, "test");

        // Insert
        db.insert_step_result(&step_result).unwrap();

        // Get
        let fetched = db.get_step_result("sr1").unwrap().unwrap();
        assert_eq!(fetched.id, "sr1");
        assert_eq!(fetched.name, "test");
        assert_eq!(fetched.job_id, "job1");
        assert_eq!(fetched.status, StepStatus::Pending);

        // Get all for run
        let results = db.get_step_results_for_run("run1").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].job_id, "job1");

        // Get all for job
        let results = db.get_step_results_for_job("job1").unwrap();
        assert_eq!(results.len(), 1);

        // Update
        let mut updated = fetched.clone();
        updated.status = StepStatus::Passed;
        updated.exit_code = Some(0);
        updated.duration_ms = Some(1234);
        db.update_step_result(&updated).unwrap();

        let fetched = db.get_step_result("sr1").unwrap().unwrap();
        assert_eq!(fetched.status, StepStatus::Passed);
        assert_eq!(fetched.exit_code, Some(0));
        assert_eq!(fetched.duration_ms, Some(1234));

        // Delete all for run
        let deleted = db.delete_step_results_for_run("run1").unwrap();
        assert_eq!(deleted, 1);
        assert!(db.get_step_result("sr1").unwrap().is_none());
    }

    #[test]
    fn test_step_result_all_statuses() {
        let db = Database::open_in_memory().unwrap();
        let (_repo_id, _run_id, job_id) = setup_repo_run_job(&db);

        let statuses = [
            StepStatus::Pending,
            StepStatus::Running,
            StepStatus::Passed,
            StepStatus::Failed,
            StepStatus::Skipped,
            StepStatus::AwaitingApproval,
        ];

        for (i, status) in statuses.iter().enumerate() {
            let mut step_result = create_test_step_result(
                &format!("sr{}", i),
                "run1",
                &job_id,
                &format!("step{}", i),
            );
            step_result.status = *status;
            db.insert_step_result(&step_result).unwrap();

            let fetched = db.get_step_result(&format!("sr{}", i)).unwrap().unwrap();
            assert_eq!(fetched.status, *status, "Status mismatch for {:?}", status);
        }
    }

    #[test]
    fn test_step_result_with_timestamps() {
        let db = Database::open_in_memory().unwrap();
        let (_repo_id, _run_id, job_id) = setup_repo_run_job(&db);

        let mut step_result = create_test_step_result("sr1", "run1", &job_id, "describe");
        step_result.status = StepStatus::Passed;
        step_result.exit_code = Some(0);
        step_result.duration_ms = Some(5432);
        step_result.started_at = Some(1704067200);
        step_result.completed_at = Some(1704067205);

        db.insert_step_result(&step_result).unwrap();

        let fetched = db.get_step_result("sr1").unwrap().unwrap();
        assert_eq!(fetched.started_at, Some(1704067200));
        assert_eq!(fetched.completed_at, Some(1704067205));
    }

    #[test]
    fn test_step_result_with_error() {
        let db = Database::open_in_memory().unwrap();
        let (_repo_id, _run_id, job_id) = setup_repo_run_job(&db);

        let mut step_result = create_test_step_result("sr1", "run1", &job_id, "test");
        step_result.status = StepStatus::Failed;
        step_result.exit_code = Some(1);
        step_result.error = Some("Tests failed: 3 failures".to_string());

        db.insert_step_result(&step_result).unwrap();

        let fetched = db.get_step_result("sr1").unwrap().unwrap();
        assert_eq!(fetched.status, StepStatus::Failed);
        assert_eq!(fetched.exit_code, Some(1));
        assert_eq!(fetched.error, Some("Tests failed: 3 failures".to_string()));
    }

    #[test]
    fn test_step_results_for_job() {
        let db = Database::open_in_memory().unwrap();
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();
        let run = create_test_run("run1", "repo1");
        db.insert_run(&run).unwrap();

        // Create two jobs
        let job1 = create_test_job_result("job1", "run1", "lint");
        let job2 = create_test_job_result("job2", "run1", "test");
        db.insert_job_result(&job1).unwrap();
        db.insert_job_result(&job2).unwrap();

        // Steps for job1
        let mut s1 = create_test_step_result("sr1", "run1", "job1", "lint-step");
        s1.step_order = 0;
        let mut s2 = create_test_step_result("sr2", "run1", "job1", "format-step");
        s2.step_order = 1;
        db.insert_step_result(&s1).unwrap();
        db.insert_step_result(&s2).unwrap();

        // Steps for job2
        let mut s3 = create_test_step_result("sr3", "run1", "job2", "test-step");
        s3.step_order = 0;
        db.insert_step_result(&s3).unwrap();

        // Query steps for job1
        let job1_steps = db.get_step_results_for_job("job1").unwrap();
        assert_eq!(job1_steps.len(), 2);
        assert_eq!(job1_steps[0].name, "lint-step");
        assert_eq!(job1_steps[1].name, "format-step");

        // Query steps for job2
        let job2_steps = db.get_step_results_for_job("job2").unwrap();
        assert_eq!(job2_steps.len(), 1);
        assert_eq!(job2_steps[0].name, "test-step");

        // Query all steps for run
        let all_steps = db.get_step_results_for_run("run1").unwrap();
        assert_eq!(all_steps.len(), 3);
    }

    // =========================================================================
    // Sync Log Tests
    // =========================================================================

    #[test]
    fn test_sync_log_operations() {
        let db = Database::open_in_memory().unwrap();
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();

        let log1 = create_test_sync_log("log1", "repo1");
        let mut log2 = create_test_sync_log("log2", "repo1");
        log2.synced_at = 1704153600; // Later timestamp

        // Insert
        db.insert_sync_log(&log1).unwrap();
        db.insert_sync_log(&log2).unwrap();

        // Get latest
        let latest = db.get_latest_sync_log("repo1").unwrap().unwrap();
        assert_eq!(latest.id, "log2");

        // List
        let logs = db.list_sync_logs("repo1", None).unwrap();
        assert_eq!(logs.len(), 2);

        // Cleanup (keep only 1)
        let deleted = db.cleanup_sync_logs("repo1", 1).unwrap();
        assert_eq!(deleted, 1);
        let remaining = db.list_sync_logs("repo1", None).unwrap();
        assert_eq!(remaining.len(), 1);
    }

    // =========================================================================
    // Cascade Delete Tests
    // =========================================================================

    #[test]
    fn test_cascade_delete_runs_jobs_and_steps() {
        let db = Database::open_in_memory().unwrap();

        // Create repo with run, job, and step results
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();

        let run = create_test_run("run1", "repo1");
        db.insert_run(&run).unwrap();

        let job = create_test_job_result("job1", "run1", "default");
        db.insert_job_result(&job).unwrap();

        let step1 = create_test_step_result("sr1", "run1", "job1", "describe");
        let step2 = create_test_step_result("sr2", "run1", "job1", "test");
        db.insert_step_result(&step1).unwrap();
        db.insert_step_result(&step2).unwrap();

        let log = create_test_sync_log("log1", "repo1");
        db.insert_sync_log(&log).unwrap();

        // Verify everything exists
        assert!(db.get_run("run1").unwrap().is_some());
        assert!(db.get_job_result("job1").unwrap().is_some());
        assert!(db.get_step_result("sr1").unwrap().is_some());
        assert!(db.get_step_result("sr2").unwrap().is_some());

        // Delete repo - should cascade to runs -> job_results -> step_results
        db.delete_repo("repo1").unwrap();

        // Everything should be gone
        assert!(db.get_run("run1").unwrap().is_none());
        assert!(db.get_job_result("job1").unwrap().is_none());
        assert!(db.get_step_result("sr1").unwrap().is_none());
        assert!(db.get_step_result("sr2").unwrap().is_none());
        assert!(db.get_latest_sync_log("repo1").unwrap().is_none());
    }

    #[test]
    fn test_cascade_delete_run_jobs_steps() {
        let db = Database::open_in_memory().unwrap();

        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();

        let run = create_test_run("run1", "repo1");
        db.insert_run(&run).unwrap();

        let job = create_test_job_result("job1", "run1", "default");
        db.insert_job_result(&job).unwrap();

        let step = create_test_step_result("sr1", "run1", "job1", "test");
        db.insert_step_result(&step).unwrap();

        // Verify everything exists
        assert!(db.get_job_result("job1").unwrap().is_some());
        assert!(db.get_step_result("sr1").unwrap().is_some());

        // Delete run - should cascade to job_results and step_results
        db.delete_run("run1").unwrap();

        // Job and step should be gone
        assert!(db.get_job_result("job1").unwrap().is_none());
        assert!(db.get_step_result("sr1").unwrap().is_none());
    }

    #[test]
    fn test_cascade_delete_job_steps() {
        let db = Database::open_in_memory().unwrap();
        let (_repo_id, _run_id, _job_id) = setup_repo_run_job(&db);

        let step = create_test_step_result("sr1", "run1", "job1", "test");
        db.insert_step_result(&step).unwrap();

        // Verify step exists
        assert!(db.get_step_result("sr1").unwrap().is_some());

        // Delete job - should cascade to step_results
        db.delete_job_results_for_run("run1").unwrap();

        // Step should be gone (cascade from job deletion)
        assert!(db.get_step_result("sr1").unwrap().is_none());
    }

    // =========================================================================
    // Not Found Error Tests
    // =========================================================================

    #[test]
    fn test_not_found_errors() {
        let db = Database::open_in_memory().unwrap();

        // Update non-existent repo
        let result = db.update_repo_last_sync("nonexistent", 123);
        assert!(matches!(result, Err(AirlockError::NotFound(_, _))));

        // Delete non-existent repo
        let result = db.delete_repo("nonexistent");
        assert!(matches!(result, Err(AirlockError::NotFound(_, _))));

        // Update non-existent run
        let result = db.update_run_current_step("nonexistent", Some("test"));
        assert!(matches!(result, Err(AirlockError::NotFound(_, _))));

        // Update non-existent step result
        let step_result = create_test_step_result("nonexistent", "run1", "job1", "test");
        let result = db.update_step_result(&step_result);
        assert!(matches!(result, Err(AirlockError::NotFound(_, _))));

        // Update non-existent job result
        let result = db.update_job_status("nonexistent", JobStatus::Running, None, None, None);
        assert!(matches!(result, Err(AirlockError::NotFound(_, _))));
    }

    // =========================================================================
    // Run List / Sort Tests
    // =========================================================================

    /// Test that list_runs returns runs sorted by recency (newest first).
    #[test]
    fn test_list_runs_sorted_by_recency() {
        let db = Database::open_in_memory().unwrap();
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();

        // Create runs with different timestamps (insert in non-chronological order)
        let mut run_middle = create_test_run("run-middle", "repo1");
        run_middle.created_at = 1704067200; // Jan 1, 2024 00:00:00 UTC
        run_middle.updated_at = run_middle.created_at;

        let mut run_oldest = create_test_run("run-oldest", "repo1");
        run_oldest.created_at = 1703980800; // Dec 31, 2023 00:00:00 UTC
        run_oldest.updated_at = run_oldest.created_at;

        let mut run_newest = create_test_run("run-newest", "repo1");
        run_newest.created_at = 1704153600; // Jan 2, 2024 00:00:00 UTC
        run_newest.updated_at = run_newest.created_at;

        // Insert runs in a scrambled order to ensure sorting is done by DB, not insertion order
        db.insert_run(&run_middle).unwrap();
        db.insert_run(&run_oldest).unwrap();
        db.insert_run(&run_newest).unwrap();

        // Retrieve runs
        let runs = db.list_runs("repo1", None).unwrap();

        // Should have all 3 runs
        assert_eq!(runs.len(), 3, "Should have 3 runs");

        // Should be sorted newest first (descending by created_at)
        assert_eq!(runs[0].id, "run-newest");
        assert_eq!(runs[1].id, "run-middle");
        assert_eq!(runs[2].id, "run-oldest");

        // Verify timestamps are in descending order
        assert!(runs[0].created_at > runs[1].created_at);
        assert!(runs[1].created_at > runs[2].created_at);
    }

    /// Test that list_runs respects the limit parameter while maintaining sort order.
    #[test]
    fn test_list_runs_limit_preserves_recency_order() {
        let db = Database::open_in_memory().unwrap();
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();

        // Insert 5 runs with different timestamps
        for i in 0..5 {
            let mut run = create_test_run(&format!("run-{}", i), "repo1");
            run.created_at = 1704067200 + (i as i64 * 86400); // Each day apart
            run.updated_at = run.created_at;
            db.insert_run(&run).unwrap();
        }

        // Get with limit of 2
        let runs = db.list_runs("repo1", Some(2)).unwrap();

        assert_eq!(runs.len(), 2, "Should return only 2 runs");

        // Should be the 2 NEWEST runs (run-4 and run-3)
        assert_eq!(runs[0].id, "run-4");
        assert_eq!(runs[1].id, "run-3");

        // Both should be newer than the runs that were cut off
        assert!(runs[0].created_at > runs[1].created_at);
    }

    // =========================================================================
    // Active Runs Tests (using job-based status)
    // =========================================================================

    /// Test active runs filtering based on job statuses
    #[test]
    fn test_list_active_runs() {
        let db = Database::open_in_memory().unwrap();
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();

        // Run with pending job (should be active)
        let run1 = create_test_run("run1", "repo1");
        db.insert_run(&run1).unwrap();
        let mut job1 = create_test_job_result("job1", "run1", "default");
        job1.status = JobStatus::Pending;
        db.insert_job_result(&job1).unwrap();

        // Run with completed job (should NOT be active)
        let run2 = create_test_run("run2", "repo1");
        db.insert_run(&run2).unwrap();
        let mut job2 = create_test_job_result("job2", "run2", "default");
        job2.status = JobStatus::Passed;
        db.insert_job_result(&job2).unwrap();

        // Run with awaiting approval job (should be active)
        let run3 = create_test_run("run3", "repo1");
        db.insert_run(&run3).unwrap();
        let mut job3 = create_test_job_result("job3", "run3", "default");
        job3.status = JobStatus::AwaitingApproval;
        db.insert_job_result(&job3).unwrap();

        // Run with no jobs yet (should be active - empty = pending)
        let run4 = create_test_run("run4", "repo1");
        db.insert_run(&run4).unwrap();

        let active = db.list_active_runs("repo1").unwrap();

        // Should include run1 (pending), run3 (awaiting_approval), run4 (no jobs)
        assert_eq!(active.len(), 3);
        let active_ids: Vec<&str> = active.iter().map(|r| r.id.as_str()).collect();
        assert!(active_ids.contains(&"run1"));
        assert!(active_ids.contains(&"run3"));
        assert!(active_ids.contains(&"run4"));
        // run2 should NOT be in the list
        assert!(!active_ids.contains(&"run2"));
    }

    /// Test that list_active_runs excludes superseded runs
    #[test]
    fn test_list_active_runs_excludes_superseded() {
        let db = Database::open_in_memory().unwrap();
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();

        // Run with pending job (should be active)
        let run1 = create_test_run("run1", "repo1");
        db.insert_run(&run1).unwrap();
        let mut job1 = create_test_job_result("job1", "run1", "default");
        job1.status = JobStatus::Pending;
        db.insert_job_result(&job1).unwrap();

        // Run with running job but superseded (should NOT be active)
        let run2 = create_test_run("run2", "repo1");
        db.insert_run(&run2).unwrap();
        let mut job2 = create_test_job_result("job2", "run2", "default");
        job2.status = JobStatus::Running;
        db.insert_job_result(&job2).unwrap();
        db.mark_run_superseded("run2").unwrap();

        // Run with no jobs but superseded (should NOT be active)
        let run3 = create_test_run("run3", "repo1");
        db.insert_run(&run3).unwrap();
        db.mark_run_superseded("run3").unwrap();

        let active = db.list_active_runs("repo1").unwrap();

        // Only run1 should be active
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "run1");
    }

    // =========================================================================
    // Compute Run Status (job-based) Tests
    // =========================================================================

    #[test]
    fn test_compute_run_status_from_jobs() {
        let db = Database::open_in_memory().unwrap();
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();

        // Run with no jobs = pending
        let run1 = create_test_run("run1", "repo1");
        db.insert_run(&run1).unwrap();
        assert_eq!(db.compute_run_status(&run1).unwrap(), "pending");

        // Run with running job
        let run2 = create_test_run("run2", "repo1");
        db.insert_run(&run2).unwrap();
        let mut job2 = create_test_job_result("job2", "run2", "default");
        job2.status = JobStatus::Running;
        db.insert_job_result(&job2).unwrap();
        assert_eq!(db.compute_run_status(&run2).unwrap(), "running");

        // Run with all passed jobs
        let run3 = create_test_run("run3", "repo1");
        db.insert_run(&run3).unwrap();
        let mut job3 = create_test_job_result("job3", "run3", "default");
        job3.status = JobStatus::Passed;
        db.insert_job_result(&job3).unwrap();
        assert_eq!(db.compute_run_status(&run3).unwrap(), "completed");

        // Run with failed job
        let run4 = create_test_run("run4", "repo1");
        db.insert_run(&run4).unwrap();
        let mut job4a = create_test_job_result("job4a", "run4", "lint");
        job4a.status = JobStatus::Passed;
        let mut job4b = create_test_job_result("job4b", "run4", "test");
        job4b.status = JobStatus::Failed;
        db.insert_job_result(&job4a).unwrap();
        db.insert_job_result(&job4b).unwrap();
        assert_eq!(db.compute_run_status(&run4).unwrap(), "failed");

        // Run with awaiting approval
        let run5 = create_test_run("run5", "repo1");
        db.insert_run(&run5).unwrap();
        let mut job5 = create_test_job_result("job5", "run5", "deploy");
        job5.status = JobStatus::AwaitingApproval;
        db.insert_job_result(&job5).unwrap();
        assert_eq!(db.compute_run_status(&run5).unwrap(), "awaiting_approval");
    }

    #[test]
    fn test_compute_run_status_superseded_overrides() {
        let db = Database::open_in_memory().unwrap();
        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();

        let run = create_test_run("run1", "repo1");
        db.insert_run(&run).unwrap();
        let mut job = create_test_job_result("job1", "run1", "default");
        job.status = JobStatus::Running;
        db.insert_job_result(&job).unwrap();

        // Supersede the run
        db.mark_run_superseded("run1").unwrap();
        let superseded_run = db.get_run("run1").unwrap().unwrap();
        assert_eq!(
            db.compute_run_status(&superseded_run).unwrap(),
            "superseded"
        );
    }

    /// Regression test: a run with some Failed and some Pending steps is NOT
    /// considered "completed" by the step-based check, but IS completed by
    /// the job-based check when all jobs have final status.
    #[test]
    fn test_is_completed_with_mixed_step_statuses() {
        let run = create_test_run("run1", "repo1");

        // Steps: one Failed, one still Pending (simulates the bug where
        // execute_single_job broke out early without marking remaining steps)
        let steps = vec![
            StepResult {
                status: StepStatus::Failed,
                ..create_test_step_result("s1", "run1", "job1", "step1")
            },
            StepResult {
                status: StepStatus::Pending,
                ..create_test_step_result("s2", "run1", "job1", "step2")
            },
        ];

        // Step-based: NOT completed (Pending step remains)
        assert!(
            !run.is_completed(&steps),
            "is_completed should be false when some steps are still Pending"
        );

        // Job-based: IS completed when the job itself reached a final status
        let jobs = vec![JobResult {
            status: JobStatus::Failed,
            ..create_test_job_result("job1", "run1", "default")
        }];
        assert!(
            run.is_completed_from_jobs(&jobs),
            "is_completed_from_jobs should be true when all jobs have final status"
        );
    }

    /// After the fix, the orphan handler should mark Pending steps as Skipped
    /// and non-final jobs as Failed/Skipped. Verify that the DB operations
    /// produce a fully completed run.
    #[test]
    fn test_orphan_cleanup_marks_pending_steps_and_jobs() {
        let db = Database::open_in_memory().unwrap();

        let repo = create_test_repo("repo1");
        db.insert_repo(&repo).unwrap();
        let run = create_test_run("run1", "repo1");
        db.insert_run(&run).unwrap();

        // Job in Running state (was executing when daemon crashed)
        let mut job = create_test_job_result("job1", "run1", "build");
        job.status = JobStatus::Running;
        db.insert_job_result(&job).unwrap();

        // Step 1: was running when daemon crashed
        let mut step1 = create_test_step_result("s1", "run1", "job1", "lint");
        step1.status = StepStatus::Running;
        db.insert_step_result(&step1).unwrap();

        // Step 2: was pending (not yet started)
        let step2 = create_test_step_result("s2", "run1", "job1", "test");
        db.insert_step_result(&step2).unwrap();

        // Simulate orphan handler: mark Running steps as Failed
        step1.status = StepStatus::Failed;
        step1.error = Some("Stage interrupted: daemon was restarted".to_string());
        db.update_step_result(&step1).unwrap();

        // Simulate orphan handler: mark Pending steps as Skipped
        let mut step2_updated = step2.clone();
        step2_updated.status = StepStatus::Skipped;
        db.update_step_result(&step2_updated).unwrap();

        // Simulate orphan handler: mark Running job as Failed
        db.update_job_status(
            "job1",
            JobStatus::Failed,
            None,
            None,
            Some("Pipeline interrupted: daemon was restarted"),
        )
        .unwrap();

        // Now verify the run is considered completed
        let steps = db.get_step_results_for_run("run1").unwrap();
        assert!(
            run.is_completed(&steps),
            "After orphan cleanup, all steps should have final status"
        );
        let jobs = db.get_job_results_for_run("run1").unwrap();
        assert!(
            run.is_completed_from_jobs(&jobs),
            "After orphan cleanup, all jobs should have final status"
        );
    }
}
