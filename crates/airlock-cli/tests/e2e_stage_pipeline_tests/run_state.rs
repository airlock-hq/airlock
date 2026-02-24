//! Tests for run state derivation from steps and jobs,
//! including superseded run behavior.

use super::helpers::*;
use airlock_core::{JobStatus, StepStatus};

// =============================================================================
// Run State Derivation (step-based)
// =============================================================================

#[test]
fn test_run_is_running_with_pending_steps() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Running);

    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Running);
    create_step_result(&db, &run_id, &job_id, "push", StepStatus::Pending);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "running");
}

#[test]
fn test_run_is_completed_all_steps_passed() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Passed);

    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "push", StepStatus::Passed);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "completed");
}

#[test]
fn test_run_is_failed_with_failed_step() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/feature/broken");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Failed);

    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Failed);
    create_step_result(&db, &run_id, &job_id, "push", StepStatus::Skipped);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "failed");
}

#[test]
fn test_run_is_awaiting_approval() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/feature/new");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::AwaitingApproval);

    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "push", StepStatus::AwaitingApproval);
    create_step_result(&db, &run_id, &job_id, "create-pr", StepStatus::Pending);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "awaiting_approval");
}

#[test]
fn test_run_with_skipped_steps() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Failed);

    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "push", StepStatus::Failed);
    create_step_result(&db, &run_id, &job_id, "create-pr", StepStatus::Skipped);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "failed");
}

#[test]
fn test_run_with_empty_steps() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "pending");
}

// =============================================================================
// Run State Derivation (job-based)
// =============================================================================

#[test]
fn test_run_derived_status_from_jobs_running() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    create_test_job(&db, &run_id, "lint", JobStatus::Passed);
    create_test_job(&db, &run_id, "test", JobStatus::Running);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "running");
}

#[test]
fn test_run_derived_status_from_jobs_completed() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    create_test_job(&db, &run_id, "lint", JobStatus::Passed);
    create_test_job(&db, &run_id, "test", JobStatus::Passed);
    create_test_job(&db, &run_id, "deploy", JobStatus::Passed);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "completed");
}

#[test]
fn test_run_derived_status_from_jobs_failed() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    create_test_job(&db, &run_id, "lint", JobStatus::Passed);
    create_test_job(&db, &run_id, "test", JobStatus::Failed);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "failed");
}

#[test]
fn test_run_derived_status_from_jobs_awaiting_approval() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    create_test_job(&db, &run_id, "lint", JobStatus::Passed);
    create_test_job(&db, &run_id, "deploy", JobStatus::AwaitingApproval);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "awaiting_approval");
}

#[test]
fn test_run_derived_status_from_jobs_skipped_counts_as_completed() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    create_test_job(&db, &run_id, "lint", JobStatus::Passed);
    create_test_job(&db, &run_id, "optional", JobStatus::Skipped);

    let run = db.get_run(&run_id).unwrap().unwrap();
    let jobs = db.get_job_results_for_run(&run_id).unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "completed");
    assert!(run.is_successful_from_jobs(&jobs));
}

// =============================================================================
// Superseded Run Behavior
// =============================================================================

#[test]
fn test_superseded_run_derived_status() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/feature/old");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Running);
    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Running);
    create_step_result(&db, &run_id, &job_id, "push", StepStatus::Pending);

    // Without supersession, it should be "running"
    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "running");

    // Mark as superseded
    db.mark_run_superseded(&run_id).unwrap();

    // Now derived_status should be "superseded"
    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "superseded");
    assert!(run.is_superseded());
}

#[test]
fn test_superseded_run_not_in_active_runs() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run1_id = create_test_run(&db, &repo_id, "refs/heads/feature/branch");
    let job1_id = create_test_job(&db, &run1_id, "default", JobStatus::Running);
    create_step_result(&db, &run1_id, &job1_id, "test", StepStatus::Running);

    let run2_id = create_test_run(&db, &repo_id, "refs/heads/feature/branch");
    let job2_id = create_test_job(&db, &run2_id, "default", JobStatus::Pending);
    create_step_result(&db, &run2_id, &job2_id, "test", StepStatus::Pending);

    // Both should be active initially
    let active = db.list_active_runs(&repo_id).unwrap();
    assert_eq!(active.len(), 2);

    // Supersede run1
    db.mark_run_superseded(&run1_id).unwrap();

    // Only run2 should be active now
    let active = db.list_active_runs(&repo_id).unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, run2_id);
}
