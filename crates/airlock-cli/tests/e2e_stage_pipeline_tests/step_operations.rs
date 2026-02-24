//! Tests for step CRUD, status transitions, approval/rejection workflows,
//! and continue-on-error behavior.

use super::helpers::*;
use airlock_core::{JobStatus, StepStatus};

// =============================================================================
// Step Operations
// =============================================================================

#[test]
fn test_step_result_crud_operations() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Running);

    // Create step results
    let step1_id = create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Pending);
    let _step2_id = create_step_result(&db, &run_id, &job_id, "test", StepStatus::Pending);

    // Verify we can retrieve them
    let steps = db.get_step_results_for_run(&run_id).unwrap();
    assert_eq!(steps.len(), 2);

    // Get individual step
    let step1 = db.get_step_result(&step1_id).unwrap().unwrap();
    assert_eq!(step1.name, "describe");
    assert_eq!(step1.status, StepStatus::Pending);
    assert_eq!(step1.job_id, job_id);

    // Update step status
    let mut updated = step1.clone();
    updated.status = StepStatus::Running;
    updated.started_at = Some(now_timestamp());
    db.update_step_result(&updated).unwrap();

    // Verify update
    let step1_updated = db.get_step_result(&step1_id).unwrap().unwrap();
    assert_eq!(step1_updated.status, StepStatus::Running);
    assert!(step1_updated.started_at.is_some());

    // Delete steps for run
    db.delete_step_results_for_run(&run_id).unwrap();
    let steps_after = db.get_step_results_for_run(&run_id).unwrap();
    assert!(steps_after.is_empty());
}

#[test]
fn test_step_status_transitions() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Running);

    // Create a pending step
    let step_id = create_step_result(&db, &run_id, &job_id, "test", StepStatus::Pending);

    // Transition: Pending -> Running
    let mut step = db.get_step_result(&step_id).unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Pending);

    step.status = StepStatus::Running;
    step.started_at = Some(now_timestamp());
    db.update_step_result(&step).unwrap();

    let step = db.get_step_result(&step_id).unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Running);

    // Transition: Running -> Passed
    let mut step = step;
    step.status = StepStatus::Passed;
    step.exit_code = Some(0);
    step.completed_at = Some(now_timestamp());
    step.duration_ms = Some(500);
    db.update_step_result(&step).unwrap();

    let step = db.get_step_result(&step_id).unwrap().unwrap();
    assert_eq!(step.status, StepStatus::Passed);
    assert_eq!(step.exit_code, Some(0));
    assert!(step.status.is_final());
}

#[test]
fn test_step_approval_workflow() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::AwaitingApproval);

    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Passed);
    let push_step_id =
        create_step_result(&db, &run_id, &job_id, "push", StepStatus::AwaitingApproval);
    create_step_result(&db, &run_id, &job_id, "create-pr", StepStatus::Pending);

    // Verify run is awaiting approval
    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "awaiting_approval");

    // Simulate approval: mark push as passed
    let mut push = db.get_step_result(&push_step_id).unwrap().unwrap();
    push.status = StepStatus::Passed;
    push.completed_at = Some(now_timestamp());
    db.update_step_result(&push).unwrap();

    // Update job status to running after approval
    db.update_job_status(
        &job_id,
        JobStatus::Running,
        Some(now_timestamp()),
        None,
        None,
    )
    .unwrap();

    // Verify run is no longer awaiting approval (now running with pending create-pr step)
    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "running");
}

#[test]
fn test_step_rejection_workflow() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Failed);

    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Passed);
    let push_step_id =
        create_step_result(&db, &run_id, &job_id, "push", StepStatus::AwaitingApproval);
    let pr_step_id = create_step_result(&db, &run_id, &job_id, "create-pr", StepStatus::Pending);

    // Simulate rejection: mark push as failed, skip remaining steps
    let mut push = db.get_step_result(&push_step_id).unwrap().unwrap();
    push.status = StepStatus::Failed;
    push.error = Some("User rejected the changes".to_string());
    push.completed_at = Some(now_timestamp());
    db.update_step_result(&push).unwrap();

    let mut pr = db.get_step_result(&pr_step_id).unwrap().unwrap();
    pr.status = StepStatus::Skipped;
    db.update_step_result(&pr).unwrap();

    // Verify run is completed and failed
    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "failed");
}

// =============================================================================
// continue-on-error Behavior
// =============================================================================

#[test]
fn test_continue_on_error_pipeline_continues() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::AwaitingApproval);

    // test fails but pipeline continues (continue-on-error=true)
    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Failed);
    create_step_result(&db, &run_id, &job_id, "push", StepStatus::AwaitingApproval);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "awaiting_approval");
}

#[test]
fn test_pipeline_stops_on_failure() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Failed);

    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Failed);
    create_step_result(&db, &run_id, &job_id, "push", StepStatus::Skipped);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "failed");
}
