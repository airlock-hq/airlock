//! Tests for full pipeline flow, multi-run scenarios,
//! database integrity, and daemon restart behavior.

use super::helpers::*;
use airlock_core::{Database, JobStatus, Repo, Run, StepResult, StepStatus};

// =============================================================================
// Full Pipeline Flow
// =============================================================================

#[test]
fn test_full_pipeline_success_flow() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/feature/add-auth");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Running);

    // Step 1: describe step running
    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Running);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Pending);
    create_step_result(&db, &run_id, &job_id, "push", StepStatus::Pending);
    create_step_result(&db, &run_id, &job_id, "create-pr", StepStatus::Pending);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "running");

    // Step 2: describe passes, test runs
    let steps = db.get_step_results_for_run(&run_id).unwrap();
    let describe_step = steps.iter().find(|s| s.name == "describe").unwrap();
    let mut updated_describe = describe_step.clone();
    updated_describe.status = StepStatus::Passed;
    updated_describe.completed_at = Some(now_timestamp());
    db.update_step_result(&updated_describe).unwrap();

    let test_step = steps.iter().find(|s| s.name == "test").unwrap();
    let mut updated_test = test_step.clone();
    updated_test.status = StepStatus::Running;
    updated_test.started_at = Some(now_timestamp());
    db.update_step_result(&updated_test).unwrap();

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "running");

    // Step 3: test passes, push reaches approval
    let steps = db.get_step_results_for_run(&run_id).unwrap();
    let test_step = steps.iter().find(|s| s.name == "test").unwrap();
    let mut updated_test = test_step.clone();
    updated_test.status = StepStatus::Passed;
    updated_test.completed_at = Some(now_timestamp());
    db.update_step_result(&updated_test).unwrap();

    let push_step = steps.iter().find(|s| s.name == "push").unwrap();
    let mut updated_push = push_step.clone();
    updated_push.status = StepStatus::AwaitingApproval;
    updated_push.started_at = Some(now_timestamp());
    db.update_step_result(&updated_push).unwrap();

    // Update job to awaiting_approval
    db.update_job_status(
        &job_id,
        JobStatus::AwaitingApproval,
        Some(now_timestamp()),
        None,
        None,
    )
    .unwrap();

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "awaiting_approval");

    // Step 4: User approves - push passes
    let steps = db.get_step_results_for_run(&run_id).unwrap();
    let push_step = steps.iter().find(|s| s.name == "push").unwrap();
    let mut updated_push = push_step.clone();
    updated_push.status = StepStatus::Passed;
    updated_push.completed_at = Some(now_timestamp());
    db.update_step_result(&updated_push).unwrap();

    // Update job back to running
    db.update_job_status(
        &job_id,
        JobStatus::Running,
        Some(now_timestamp()),
        None,
        None,
    )
    .unwrap();

    // Step 5: create-pr step passes
    let pr_step = steps.iter().find(|s| s.name == "create-pr").unwrap();
    let mut updated_pr = pr_step.clone();
    updated_pr.status = StepStatus::Passed;
    updated_pr.completed_at = Some(now_timestamp());
    db.update_step_result(&updated_pr).unwrap();

    // Update job to passed
    db.update_job_status(
        &job_id,
        JobStatus::Passed,
        Some(now_timestamp()),
        Some(now_timestamp()),
        None,
    )
    .unwrap();

    // Verify final state
    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "completed");
}

#[test]
fn test_pipeline_with_multiple_runs() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    // Create multiple runs
    let run1_id = create_test_run(&db, &repo_id, "refs/heads/feature/a");
    let run2_id = create_test_run(&db, &repo_id, "refs/heads/feature/b");
    let run3_id = create_test_run(&db, &repo_id, "refs/heads/main");

    // Run 1: completed successfully
    let job1_id = create_test_job(&db, &run1_id, "default", JobStatus::Passed);
    create_step_result(&db, &run1_id, &job1_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run1_id, &job1_id, "test", StepStatus::Passed);

    // Run 2: failed
    let job2_id = create_test_job(&db, &run2_id, "default", JobStatus::Failed);
    create_step_result(&db, &run2_id, &job2_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run2_id, &job2_id, "test", StepStatus::Failed);

    // Run 3: awaiting approval
    let job3_id = create_test_job(&db, &run3_id, "default", JobStatus::AwaitingApproval);
    create_step_result(&db, &run3_id, &job3_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run3_id, &job3_id, "test", StepStatus::Passed);
    create_step_result(
        &db,
        &run3_id,
        &job3_id,
        "push",
        StepStatus::AwaitingApproval,
    );

    // Verify each run's state
    let run1 = db.get_run(&run1_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run1).unwrap(), "completed");

    let run2 = db.get_run(&run2_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run2).unwrap(), "failed");

    let run3 = db.get_run(&run3_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run3).unwrap(), "awaiting_approval");
}

// =============================================================================
// Database Integrity
// =============================================================================

#[test]
fn test_step_results_are_tied_to_run() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Running);
    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Passed);

    let steps = db.get_step_results_for_run(&run_id).unwrap();
    assert_eq!(steps.len(), 2);

    db.delete_step_results_for_run(&run_id).unwrap();
    let steps_after = db.get_step_results_for_run(&run_id).unwrap();
    assert!(steps_after.is_empty());

    // Run should still exist
    let run = db.get_run(&run_id).unwrap();
    assert!(run.is_some());
}

#[test]
fn test_job_results_are_tied_to_run() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    create_test_job(&db, &run_id, "lint", JobStatus::Passed);
    create_test_job(&db, &run_id, "test", JobStatus::Running);

    let jobs = db.get_job_results_for_run(&run_id).unwrap();
    assert_eq!(jobs.len(), 2);

    db.delete_job_results_for_run(&run_id).unwrap();
    let jobs_after = db.get_job_results_for_run(&run_id).unwrap();
    assert!(jobs_after.is_empty());

    // Run should still exist
    let run = db.get_run(&run_id).unwrap();
    assert!(run.is_some());
}

#[test]
fn test_step_results_cascade_delete_with_jobs() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Running);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "lint", StepStatus::Passed);

    // Steps exist
    let steps = db.get_step_results_for_run(&run_id).unwrap();
    assert_eq!(steps.len(), 2);

    // Delete job -> should cascade delete steps (via FK)
    db.delete_job_results_for_run(&run_id).unwrap();
    let steps_after = db.get_step_results_for_run(&run_id).unwrap();
    assert!(
        steps_after.is_empty(),
        "Steps should be deleted when parent job is deleted"
    );
}

#[test]
fn test_run_fields_persistence() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = format!("run_{}", uuid::Uuid::new_v4());
    let now = now_timestamp();
    let run = Run {
        id: run_id.clone(),
        repo_id: repo_id.clone(),
        ref_updates: vec![],
        error: None,
        superseded: false,
        created_at: now,
        branch: "refs/heads/feature/test".to_string(),
        base_sha: "abc123".to_string(),
        head_sha: "def456".to_string(),
        current_step: Some("describe".to_string()),
        workflow_file: "main.yml".to_string(),
        workflow_name: Some("Main Pipeline".to_string()),
        updated_at: now,
    };
    db.insert_run(&run).unwrap();

    let retrieved = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(retrieved.id, run_id);
    assert_eq!(retrieved.repo_id, repo_id);
    assert_eq!(retrieved.branch, "refs/heads/feature/test");
    assert_eq!(retrieved.base_sha, "abc123");
    assert_eq!(retrieved.head_sha, "def456");
    assert_eq!(retrieved.current_step, Some("describe".to_string()));
    assert_eq!(retrieved.workflow_file, "main.yml");
    assert_eq!(retrieved.workflow_name, Some("Main Pipeline".to_string()));
    assert_eq!(retrieved.created_at, now);
    assert_eq!(retrieved.updated_at, now);
}

#[test]
fn test_step_result_error_field() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Running);

    let step_id = format!("step_{}", uuid::Uuid::new_v4());
    let error_msg = "Test command failed: exit code 1\nstderr: assertion failed";
    let result = StepResult {
        id: step_id.clone(),
        run_id: run_id.to_string(),
        job_id: job_id.to_string(),
        name: "test".to_string(),
        status: StepStatus::Failed,
        step_order: 0,
        exit_code: Some(1),
        duration_ms: Some(5000),
        error: Some(error_msg.to_string()),
        started_at: Some(now_timestamp()),
        completed_at: Some(now_timestamp()),
    };
    db.insert_step_result(&result).unwrap();

    let retrieved = db.get_step_result(&step_id).unwrap().unwrap();
    assert_eq!(retrieved.status, StepStatus::Failed);
    assert_eq!(retrieved.exit_code, Some(1));
    assert_eq!(retrieved.error, Some(error_msg.to_string()));
}

// =============================================================================
// Repo Operations
// =============================================================================

#[test]
fn test_runs_are_scoped_to_repo() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    create_test_run(&db, &repo_id, "refs/heads/main");
    create_test_run(&db, &repo_id, "refs/heads/feature");

    let other_repo_id = "other_repo_123";
    let other_repo = Repo {
        id: other_repo_id.to_string(),
        working_path: std::path::PathBuf::from("/tmp/other/repo"),
        upstream_url: "https://github.com/other/repo.git".to_string(),
        gate_path: std::path::PathBuf::from("/tmp/other/gate"),
        last_sync: None,
        created_at: now_timestamp(),
    };
    db.insert_repo(&other_repo).unwrap();

    let other_run_id = create_test_run(&db, other_repo_id, "refs/heads/main");
    let other_job_id = create_test_job(&db, &other_run_id, "default", JobStatus::Passed);
    create_step_result(
        &db,
        &other_run_id,
        &other_job_id,
        "test",
        StepStatus::Passed,
    );

    let runs = db.list_runs(&repo_id, None).unwrap();
    assert_eq!(runs.len(), 2);
    for run in &runs {
        assert_eq!(run.repo_id, repo_id);
    }

    let other_runs = db.list_runs(other_repo_id, None).unwrap();
    assert_eq!(other_runs.len(), 1);
    assert_eq!(other_runs[0].repo_id, other_repo_id);
}

// =============================================================================
// Daemon Restart Behavior
// =============================================================================

/// Simulates the daemon's orphan handling logic on startup.
fn simulate_daemon_orphan_handling(db: &Database) {
    let all_runs = db.list_all_runs(None).unwrap();

    for run in &all_runs {
        let steps = db.get_step_results_for_run(&run.id).unwrap();

        if run.is_completed(&steps) {
            continue;
        }

        if run.is_awaiting_approval(&steps) {
            continue;
        }

        let had_running_step = steps.iter().any(|s| s.status == StepStatus::Running);

        if had_running_step {
            for step in &steps {
                if step.status == StepStatus::Running {
                    let mut failed_step = step.clone();
                    failed_step.status = StepStatus::Failed;
                    failed_step.error = Some(
                        "Step interrupted: daemon was restarted while step was running".to_string(),
                    );
                    db.update_step_result(&failed_step).unwrap();
                }
            }

            db.update_run_error(
                &run.id,
                Some("Pipeline interrupted: daemon was restarted while run was in progress"),
            )
            .unwrap();
        } else {
            db.update_run_error(
                &run.id,
                Some("Pipeline interrupted: daemon was restarted while run was in progress"),
            )
            .unwrap();
        }
    }
}

#[test]
fn test_daemon_restart_preserves_awaiting_approval_runs() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/feature/new");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::AwaitingApproval);
    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "push", StepStatus::AwaitingApproval);
    create_step_result(&db, &run_id, &job_id, "create-pr", StepStatus::Pending);

    let run = db.get_run(&run_id).unwrap().unwrap();
    let steps = db.get_step_results_for_run(&run_id).unwrap();
    assert!(run.is_awaiting_approval(&steps));

    simulate_daemon_orphan_handling(&db);

    let run_after = db.get_run(&run_id).unwrap().unwrap();
    let steps_after = db.get_step_results_for_run(&run_id).unwrap();

    assert!(
        run_after.is_awaiting_approval(&steps_after),
        "Run should still be awaiting approval after daemon restart"
    );

    let push_step = steps_after.iter().find(|s| s.name == "push").unwrap();
    assert_eq!(push_step.status, StepStatus::AwaitingApproval);
}

#[test]
fn test_daemon_restart_marks_running_step_as_failed() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/feature/active");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Running);
    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Running);
    create_step_result(&db, &run_id, &job_id, "push", StepStatus::Pending);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "running");

    simulate_daemon_orphan_handling(&db);

    let run_after = db.get_run(&run_id).unwrap().unwrap();
    let steps_after = db.get_step_results_for_run(&run_id).unwrap();

    assert!(
        run_after.is_failed(&steps_after),
        "Run should be marked as failed after daemon restart"
    );

    let test_step = steps_after.iter().find(|s| s.name == "test").unwrap();
    assert_eq!(test_step.status, StepStatus::Failed);
    assert!(test_step.error.is_some());
    assert!(test_step
        .error
        .as_ref()
        .unwrap()
        .contains("daemon was restarted"));
}

#[test]
fn test_daemon_restart_leaves_completed_runs_alone() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/feature/done");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "push", StepStatus::Passed);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "completed");

    simulate_daemon_orphan_handling(&db);

    let run_after = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run_after).unwrap(), "completed");
}

#[test]
fn test_daemon_restart_marks_pending_only_run_as_failed() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/feature/new");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Pending);
    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Pending);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Pending);

    let run = db.get_run(&run_id).unwrap().unwrap();
    let steps = db.get_step_results_for_run(&run_id).unwrap();
    assert!(run.is_running(&steps));

    simulate_daemon_orphan_handling(&db);

    let run_after = db.get_run(&run_id).unwrap().unwrap();
    assert!(run_after.error.is_some());
    assert!(run_after.error.as_ref().unwrap().contains("interrupted"));
}

#[test]
fn test_daemon_restart_with_multiple_runs_different_states() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    // Run 1: Awaiting approval (should be preserved)
    let run1_id = create_test_run(&db, &repo_id, "refs/heads/feature/approval");
    let job1_id = create_test_job(&db, &run1_id, "default", JobStatus::AwaitingApproval);
    create_step_result(&db, &run1_id, &job1_id, "test", StepStatus::Passed);
    create_step_result(
        &db,
        &run1_id,
        &job1_id,
        "push",
        StepStatus::AwaitingApproval,
    );

    // Run 2: Actively running (should be marked failed)
    let run2_id = create_test_run(&db, &repo_id, "refs/heads/feature/running");
    let job2_id = create_test_job(&db, &run2_id, "default", JobStatus::Running);
    create_step_result(&db, &run2_id, &job2_id, "test", StepStatus::Running);

    // Run 3: Completed (should be left alone)
    let run3_id = create_test_run(&db, &repo_id, "refs/heads/feature/done");
    let job3_id = create_test_job(&db, &run3_id, "default", JobStatus::Passed);
    create_step_result(&db, &run3_id, &job3_id, "test", StepStatus::Passed);
    create_step_result(&db, &run3_id, &job3_id, "push", StepStatus::Passed);

    simulate_daemon_orphan_handling(&db);

    // Run 1: Still awaiting approval
    let run1 = db.get_run(&run1_id).unwrap().unwrap();
    let steps1 = db.get_step_results_for_run(&run1_id).unwrap();
    assert!(run1.is_awaiting_approval(&steps1));

    // Run 2: Now failed
    let run2 = db.get_run(&run2_id).unwrap().unwrap();
    let steps2 = db.get_step_results_for_run(&run2_id).unwrap();
    assert!(run2.is_failed(&steps2));

    // Run 3: Still completed
    let run3 = db.get_run(&run3_id).unwrap().unwrap();
    let steps3 = db.get_step_results_for_run(&run3_id).unwrap();
    assert!(run3.is_successful(&steps3));
}
