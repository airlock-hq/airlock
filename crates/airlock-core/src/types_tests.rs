use super::*;

// =========================================================================
// StepStatus Tests
// =========================================================================

#[test]
fn test_step_status_serialization() {
    let test_cases = [
        (StepStatus::Pending, "\"pending\""),
        (StepStatus::Running, "\"running\""),
        (StepStatus::Passed, "\"passed\""),
        (StepStatus::Failed, "\"failed\""),
        (StepStatus::Skipped, "\"skipped\""),
        (StepStatus::AwaitingApproval, "\"awaiting_approval\""),
    ];

    for (status, expected_json) in test_cases {
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, expected_json, "Serialization failed for {:?}", status);

        let parsed: StepStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, status, "Deserialization failed for {:?}", status);
    }
}

#[test]
fn test_step_status_is_final() {
    assert!(!StepStatus::Pending.is_final());
    assert!(!StepStatus::Running.is_final());
    assert!(StepStatus::Passed.is_final());
    assert!(StepStatus::Failed.is_final());
    assert!(StepStatus::Skipped.is_final());
    assert!(!StepStatus::AwaitingApproval.is_final());
}

// =========================================================================
// StepDefinition Tests
// =========================================================================

#[test]
fn test_step_definition_serialization_and_defaults() {
    // Full roundtrip with explicit fields
    let step = StepDefinition {
        name: "test".to_string(),
        run: Some("npm test".to_string()),
        uses: None,
        shell: Some("bash".to_string()),
        continue_on_error: true,
        require_approval: false,
        timeout: None,
    };

    let json = serde_json::to_string(&step).unwrap();
    let parsed: StepDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.name, "test");
    assert_eq!(parsed.run, Some("npm test".to_string()));
    assert_eq!(parsed.shell, Some("bash".to_string()));
    assert!(parsed.continue_on_error);
    assert!(!parsed.require_approval);

    // Defaults applied when fields are missing
    let minimal: StepDefinition =
        serde_json::from_str(r#"{"name": "build", "run": "cargo build"}"#).unwrap();
    assert_eq!(minimal.name, "build");
    assert_eq!(minimal.shell, None);
    assert!(!minimal.continue_on_error);
    assert!(!minimal.require_approval);

    // Reusable action reference (uses instead of run)
    let reusable: StepDefinition = serde_json::from_str(
        r#"{"name": "lint", "uses": "airlock-hq/airlock/defaults/eslint@v1"}"#,
    )
    .unwrap();
    assert!(reusable.run.is_none());
    assert_eq!(
        reusable.uses,
        Some("airlock-hq/airlock/defaults/eslint@v1".to_string())
    );
    assert!(reusable.is_reusable());
}

#[test]
fn test_step_definition_effective_run() {
    // Step with run command
    let step = StepDefinition {
        name: "test".to_string(),
        run: Some("npm test".to_string()),
        uses: None,
        shell: None,
        continue_on_error: false,
        require_approval: false,
        timeout: None,
    };
    assert_eq!(step.effective_run(), Some("npm test"));

    // Step without run command (using reusable action)
    let step = StepDefinition {
        name: "lint".to_string(),
        run: None,
        uses: Some("owner/repo/lint@v1".to_string()),
        shell: None,
        continue_on_error: false,
        require_approval: false,
        timeout: None,
    };
    assert_eq!(step.effective_run(), None);
    assert!(step.is_reusable());
}

// =========================================================================
// StepResult Tests
// =========================================================================

#[test]
fn test_step_result_serialization() {
    // Full roundtrip
    let result = StepResult {
        id: "step-result-1".to_string(),
        run_id: "run-1".to_string(),
        job_id: "job-1".to_string(),
        name: "test".to_string(),
        status: StepStatus::Passed,
        step_order: 0,
        exit_code: Some(0),
        duration_ms: Some(1234),
        error: None,
        started_at: Some(1704067200),
        completed_at: Some(1704067201),
    };

    let json = serde_json::to_string(&result).unwrap();
    let parsed: StepResult = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.id, "step-result-1");
    assert_eq!(parsed.run_id, "run-1");
    assert_eq!(parsed.job_id, "job-1");
    assert_eq!(parsed.name, "test");
    assert_eq!(parsed.status, StepStatus::Passed);
    assert_eq!(parsed.exit_code, Some(0));
    assert_eq!(parsed.duration_ms, Some(1234));

    // Minimal fields: optional fields default to None
    let minimal: StepResult = serde_json::from_str(
        r#"{"id": "sr-1", "run_id": "run-1", "name": "test", "status": "pending"}"#,
    )
    .unwrap();
    assert_eq!(minimal.id, "sr-1");
    assert_eq!(minimal.status, StepStatus::Pending);
    assert!(minimal.exit_code.is_none());
    assert!(minimal.duration_ms.is_none());
    assert!(minimal.error.is_none());
}

// =========================================================================
// Run Tests (with derived status)
// =========================================================================

fn create_test_run() -> Run {
    Run {
        id: "run-1".to_string(),
        repo_id: "repo-1".to_string(),
        ref_updates: vec![],
        branch: "feature/test".to_string(),
        base_sha: "abc123".to_string(),
        head_sha: "def456".to_string(),
        current_step: None,
        error: None,
        superseded: false,
        workflow_file: String::new(),
        workflow_name: None,
        created_at: 1704067200,
        updated_at: 1704067200,
    }
}

fn create_step_result(name: &str, status: StepStatus) -> StepResult {
    StepResult {
        id: format!("sr-{}", name),
        run_id: "run-1".to_string(),
        job_id: String::new(),
        name: name.to_string(),
        status,
        step_order: 0,
        exit_code: None,
        duration_ms: None,
        error: None,
        started_at: None,
        completed_at: None,
    }
}

#[test]
fn test_run_is_running() {
    let run = create_test_run();

    // Empty steps = not running (pending)
    assert!(!run.is_running(&[]));

    // At least one pending step = running
    let steps = vec![create_step_result("test", StepStatus::Pending)];
    assert!(run.is_running(&steps));

    // At least one running step = running
    let steps = vec![
        create_step_result("build", StepStatus::Passed),
        create_step_result("test", StepStatus::Running),
    ];
    assert!(run.is_running(&steps));

    // Awaiting approval = running
    let steps = vec![create_step_result("review", StepStatus::AwaitingApproval)];
    assert!(run.is_running(&steps));

    // All final = not running
    let steps = vec![
        create_step_result("build", StepStatus::Passed),
        create_step_result("test", StepStatus::Passed),
    ];
    assert!(!run.is_running(&steps));
}

#[test]
fn test_run_is_completed() {
    let run = create_test_run();

    // Empty steps = not completed
    assert!(!run.is_completed(&[]));

    // Some pending = not completed
    let steps = vec![
        create_step_result("build", StepStatus::Passed),
        create_step_result("test", StepStatus::Pending),
    ];
    assert!(!run.is_completed(&steps));

    // Awaiting approval = not completed
    let steps = vec![create_step_result("review", StepStatus::AwaitingApproval)];
    assert!(!run.is_completed(&steps));

    // All final = completed
    let steps = vec![
        create_step_result("build", StepStatus::Passed),
        create_step_result("test", StepStatus::Failed),
    ];
    assert!(run.is_completed(&steps));
}

#[test]
fn test_run_is_failed() {
    let run = create_test_run();

    // No failed steps = not failed
    let steps = vec![
        create_step_result("build", StepStatus::Passed),
        create_step_result("test", StepStatus::Passed),
    ];
    assert!(!run.is_failed(&steps));

    // At least one failed = failed
    let steps = vec![
        create_step_result("build", StepStatus::Passed),
        create_step_result("test", StepStatus::Failed),
    ];
    assert!(run.is_failed(&steps));
}

#[test]
fn test_run_is_successful() {
    let run = create_test_run();

    // Empty steps = not successful
    assert!(!run.is_successful(&[]));

    // All passed = successful
    let steps = vec![
        create_step_result("build", StepStatus::Passed),
        create_step_result("test", StepStatus::Passed),
    ];
    assert!(run.is_successful(&steps));

    // Passed and skipped = successful
    let steps = vec![
        create_step_result("build", StepStatus::Passed),
        create_step_result("test", StepStatus::Skipped),
    ];
    assert!(run.is_successful(&steps));

    // Any failed = not successful
    let steps = vec![
        create_step_result("build", StepStatus::Passed),
        create_step_result("test", StepStatus::Failed),
    ];
    assert!(!run.is_successful(&steps));
}

#[test]
fn test_run_is_awaiting_approval() {
    let run = create_test_run();

    // No awaiting = false
    let steps = vec![create_step_result("test", StepStatus::Passed)];
    assert!(!run.is_awaiting_approval(&steps));

    // At least one awaiting = true
    let steps = vec![
        create_step_result("test", StepStatus::Passed),
        create_step_result("review", StepStatus::AwaitingApproval),
    ];
    assert!(run.is_awaiting_approval(&steps));
}

#[test]
fn test_run_derived_status() {
    let run = create_test_run();

    // Empty = pending
    assert_eq!(run.derived_status(&[]), "pending");

    // Running
    let steps = vec![create_step_result("test", StepStatus::Running)];
    assert_eq!(run.derived_status(&steps), "running");

    // Awaiting approval (takes precedence over running)
    let steps = vec![
        create_step_result("test", StepStatus::Passed),
        create_step_result("review", StepStatus::AwaitingApproval),
    ];
    assert_eq!(run.derived_status(&steps), "awaiting_approval");

    // Failed (takes precedence over completed)
    let steps = vec![
        create_step_result("build", StepStatus::Passed),
        create_step_result("test", StepStatus::Failed),
    ];
    assert_eq!(run.derived_status(&steps), "failed");

    // Completed/successful
    let steps = vec![
        create_step_result("build", StepStatus::Passed),
        create_step_result("test", StepStatus::Passed),
    ];
    assert_eq!(run.derived_status(&steps), "completed");
}

#[test]
fn test_is_superseded() {
    let mut run = create_test_run();

    // Not superseded by default
    assert!(!run.is_superseded());

    // Not superseded even with an error
    run.error = Some("Some other error".to_string());
    assert!(!run.is_superseded());

    // Superseded when the flag is set
    run.superseded = true;
    assert!(run.is_superseded());
}

#[test]
fn test_derived_status_superseded_overrides_running() {
    let mut run = create_test_run();
    run.superseded = true;

    // Even with running/pending steps, superseded takes priority
    let steps = vec![
        create_step_result("build", StepStatus::Passed),
        create_step_result("test", StepStatus::Running),
        create_step_result("review", StepStatus::Pending),
    ];
    assert_eq!(run.derived_status(&steps), "superseded");
}

#[test]
fn test_derived_status_superseded_overrides_awaiting() {
    let mut run = create_test_run();
    run.superseded = true;

    // Even with awaiting approval, superseded takes priority
    let steps = vec![
        create_step_result("build", StepStatus::Passed),
        create_step_result("review", StepStatus::AwaitingApproval),
    ];
    assert_eq!(run.derived_status(&steps), "superseded");
}

#[test]
fn test_derived_status_superseded_overrides_pending() {
    let mut run = create_test_run();
    run.superseded = true;

    // Even with no steps (normally "pending"), superseded takes priority
    assert_eq!(run.derived_status(&[]), "superseded");
}

#[test]
fn test_run_serialization() {
    let run = Run {
        id: "run-1".to_string(),
        repo_id: "repo-1".to_string(),
        ref_updates: vec![],
        branch: "feature/test".to_string(),
        base_sha: "abc123".to_string(),
        head_sha: "def456".to_string(),
        current_step: Some("test".to_string()),
        error: None,
        superseded: false,
        workflow_file: String::new(),
        workflow_name: None,
        created_at: 1704067200,
        updated_at: 1704067201,
    };

    let json = serde_json::to_string(&run).unwrap();
    assert!(json.contains("\"branch\":\"feature/test\""));
    assert!(json.contains("\"base_sha\":\"abc123\""));
    assert!(json.contains("\"head_sha\":\"def456\""));
    assert!(json.contains("\"current_step\":\"test\""));

    let parsed: Run = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.id, "run-1");
    assert_eq!(parsed.branch, "feature/test");
    assert_eq!(parsed.current_step, Some("test".to_string()));
}

// =========================================================================
// Job-based derived status tests
// =========================================================================

fn create_job_result(key: &str, status: JobStatus) -> JobResult {
    JobResult {
        id: format!("jr-{}", key),
        run_id: "run-1".to_string(),
        job_key: key.to_string(),
        name: None,
        status,
        job_order: 0,
        started_at: None,
        completed_at: None,
        error: None,
    }
}

#[test]
fn test_derived_status_from_jobs_empty() {
    let run = create_test_run();
    assert_eq!(run.derived_status_from_jobs(&[]), "pending");
}

#[test]
fn test_derived_status_from_jobs_error_no_jobs() {
    let mut run = create_test_run();
    run.error = Some("workflow loading failed".to_string());
    assert_eq!(run.derived_status_from_jobs(&[]), "failed");
}

#[test]
fn test_derived_status_from_jobs_running() {
    let run = create_test_run();
    let jobs = vec![
        create_job_result("lint", JobStatus::Passed),
        create_job_result("test", JobStatus::Running),
    ];
    assert_eq!(run.derived_status_from_jobs(&jobs), "running");
}

#[test]
fn test_derived_status_from_jobs_awaiting_approval() {
    let run = create_test_run();
    // AwaitingApproval takes precedence when no Running jobs
    let jobs = vec![
        create_job_result("lint", JobStatus::Passed),
        create_job_result("deploy", JobStatus::AwaitingApproval),
    ];
    assert_eq!(run.derived_status_from_jobs(&jobs), "awaiting_approval");
}

#[test]
fn test_derived_status_from_jobs_running_overrides_awaiting() {
    let run = create_test_run();
    // Running takes precedence over AwaitingApproval
    let jobs = vec![
        create_job_result("lint", JobStatus::Running),
        create_job_result("deploy", JobStatus::AwaitingApproval),
    ];
    assert_eq!(run.derived_status_from_jobs(&jobs), "running");
}

#[test]
fn test_derived_status_from_jobs_failed() {
    let run = create_test_run();
    let jobs = vec![
        create_job_result("lint", JobStatus::Passed),
        create_job_result("test", JobStatus::Failed),
    ];
    assert_eq!(run.derived_status_from_jobs(&jobs), "failed");
}

#[test]
fn test_derived_status_from_jobs_completed() {
    let run = create_test_run();
    let jobs = vec![
        create_job_result("lint", JobStatus::Passed),
        create_job_result("test", JobStatus::Passed),
    ];
    assert_eq!(run.derived_status_from_jobs(&jobs), "completed");
}

#[test]
fn test_derived_status_from_jobs_completed_with_skipped() {
    let run = create_test_run();
    let jobs = vec![
        create_job_result("lint", JobStatus::Passed),
        create_job_result("test", JobStatus::Skipped),
    ];
    assert_eq!(run.derived_status_from_jobs(&jobs), "completed");
}

#[test]
fn test_derived_status_from_jobs_pending() {
    let run = create_test_run();
    let jobs = vec![
        create_job_result("lint", JobStatus::Passed),
        create_job_result("test", JobStatus::Pending),
    ];
    // Pending jobs mean the run is still running
    assert_eq!(run.derived_status_from_jobs(&jobs), "running");
}

#[test]
fn test_derived_status_from_jobs_superseded() {
    let mut run = create_test_run();
    run.superseded = true;
    let jobs = vec![create_job_result("lint", JobStatus::Running)];
    assert_eq!(run.derived_status_from_jobs(&jobs), "superseded");
}

#[test]
fn test_is_running_from_jobs() {
    let run = create_test_run();
    assert!(!run.is_running_from_jobs(&[]));
    assert!(run.is_running_from_jobs(&[create_job_result("a", JobStatus::Pending)]));
    assert!(run.is_running_from_jobs(&[create_job_result("a", JobStatus::Running)]));
    assert!(run.is_running_from_jobs(&[create_job_result("a", JobStatus::AwaitingApproval)]));
    assert!(!run.is_running_from_jobs(&[create_job_result("a", JobStatus::Passed)]));
    assert!(!run.is_running_from_jobs(&[create_job_result("a", JobStatus::Failed)]));
}

#[test]
fn test_is_completed_from_jobs() {
    let run = create_test_run();
    assert!(!run.is_completed_from_jobs(&[]));
    assert!(!run.is_completed_from_jobs(&[create_job_result("a", JobStatus::Pending)]));
    assert!(run.is_completed_from_jobs(&[create_job_result("a", JobStatus::Passed)]));
    assert!(run.is_completed_from_jobs(&[
        create_job_result("a", JobStatus::Passed),
        create_job_result("b", JobStatus::Failed),
    ]));
}

#[test]
fn test_is_failed_from_jobs() {
    let run = create_test_run();
    assert!(!run.is_failed_from_jobs(&[create_job_result("a", JobStatus::Passed)]));
    assert!(run.is_failed_from_jobs(&[create_job_result("a", JobStatus::Failed)]));
}

#[test]
fn test_is_successful_from_jobs() {
    let run = create_test_run();
    assert!(!run.is_successful_from_jobs(&[]));
    assert!(run.is_successful_from_jobs(&[create_job_result("a", JobStatus::Passed)]));
    assert!(run.is_successful_from_jobs(&[
        create_job_result("a", JobStatus::Passed),
        create_job_result("b", JobStatus::Skipped),
    ]));
    assert!(!run.is_successful_from_jobs(&[
        create_job_result("a", JobStatus::Passed),
        create_job_result("b", JobStatus::Failed),
    ]));
}

#[test]
fn test_job_status_is_final() {
    assert!(!JobStatus::Pending.is_final());
    assert!(!JobStatus::Running.is_final());
    assert!(JobStatus::Passed.is_final());
    assert!(JobStatus::Failed.is_final());
    assert!(JobStatus::Skipped.is_final());
    assert!(!JobStatus::AwaitingApproval.is_final());
}
