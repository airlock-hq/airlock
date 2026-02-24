//! Tests for workflow YAML parsing, branch matching, DAG validation,
//! multi-job workflows, and job status interactions.

use super::helpers::*;
use airlock_core::{
    branch_matches_trigger, filter_workflows_for_branch, parse_workflow_config, validate_job_dag,
    DagValidationError, JobConfig, JobStatus, OneOrMany, PushTrigger, StepStatus, TriggerConfig,
    WorkflowConfig,
};
use indexmap::IndexMap;

// =============================================================================
// Workflow Config Parsing
// =============================================================================

#[test]
fn test_workflow_config_parsing_main_pipeline() {
    let yaml = r#"
name: Main Pipeline

on:
  push:
    branches: ['**']

jobs:
  default:
    name: Lint, Test & Deploy
    steps:
      - name: lint
        uses: airlock-hq/airlock/defaults/lint@main
      - name: test
        run: cargo test
        continue-on-error: true
      - name: push
        uses: airlock-hq/airlock/defaults/push@main
        require-approval: true
"#;
    let config: WorkflowConfig = parse_workflow_config(yaml).unwrap();
    assert_eq!(config.name, Some("Main Pipeline".to_string()));
    assert_eq!(config.jobs.len(), 1);

    let default_job = config.jobs.get("default").unwrap();
    assert_eq!(default_job.name, Some("Lint, Test & Deploy".to_string()));
    assert_eq!(default_job.steps.len(), 3);
    assert!(default_job.steps[1].continue_on_error);
    assert!(default_job.steps[2].require_approval);
}

#[test]
fn test_workflow_config_parsing_parallel_ci() {
    let yaml = r#"
name: Parallel CI

on:
  push:
    branches: ['**']

jobs:
  lint:
    name: Lint & Format
    steps:
      - name: lint
        run: cargo clippy

  test:
    name: Test
    steps:
      - name: test
        run: cargo test

  deploy:
    name: Deploy
    needs: [lint, test]
    steps:
      - name: push
        run: echo push
"#;
    let config: WorkflowConfig = parse_workflow_config(yaml).unwrap();
    assert_eq!(config.jobs.len(), 3);

    // Verify DAG
    let waves = validate_job_dag(&config.jobs).unwrap();
    assert_eq!(waves.len(), 2);
    assert_eq!(waves[0].len(), 2); // lint + test in parallel
    assert!(waves[0].contains(&"lint".to_string()));
    assert!(waves[0].contains(&"test".to_string()));
    assert_eq!(waves[1], vec!["deploy"]); // deploy after both
}

// =============================================================================
// Branch Matching
// =============================================================================

#[test]
fn test_branch_matching_workflow_filters() {
    // Match all branches
    let trigger_all = Some(TriggerConfig {
        push: Some(PushTrigger {
            branches: vec!["**".to_string()],
            branches_ignore: vec![],
        }),
    });
    assert!(branch_matches_trigger("main", &trigger_all));
    assert!(branch_matches_trigger("feature/foo", &trigger_all));
    assert!(branch_matches_trigger("a/b/c/d", &trigger_all));

    // Match only hotfix branches
    let trigger_hotfix = Some(TriggerConfig {
        push: Some(PushTrigger {
            branches: vec!["hotfix/**".to_string()],
            branches_ignore: vec![],
        }),
    });
    assert!(branch_matches_trigger("hotfix/urgent", &trigger_hotfix));
    assert!(branch_matches_trigger("hotfix/v1/fix", &trigger_hotfix));
    assert!(!branch_matches_trigger("feature/new", &trigger_hotfix));
    assert!(!branch_matches_trigger("main", &trigger_hotfix));

    // Match all except experimental
    let trigger_no_exp = Some(TriggerConfig {
        push: Some(PushTrigger {
            branches: vec!["**".to_string()],
            branches_ignore: vec!["experimental/**".to_string()],
        }),
    });
    assert!(branch_matches_trigger("main", &trigger_no_exp));
    assert!(branch_matches_trigger("feature/foo", &trigger_no_exp));
    assert!(!branch_matches_trigger(
        "experimental/test",
        &trigger_no_exp
    ));

    // No trigger -> matches all
    assert!(branch_matches_trigger("anything", &None));
}

#[test]
fn test_workflow_that_does_not_match_branch() {
    let main_yaml = r#"
name: Main
on:
  push:
    branches: ['**']
jobs:
  default:
    steps:
      - name: test
        run: cargo test
"#;
    let hotfix_yaml = r#"
name: Hotfix
on:
  push:
    branches: ['hotfix/**']
jobs:
  default:
    steps:
      - name: push
        run: echo push
"#;

    let main_config: WorkflowConfig = parse_workflow_config(main_yaml).unwrap();
    let hotfix_config: WorkflowConfig = parse_workflow_config(hotfix_yaml).unwrap();

    let workflows = vec![
        ("main.yml".to_string(), main_config),
        ("hotfix.yml".to_string(), hotfix_config),
    ];

    // Push to "feature/foo" -> only main.yml matches
    let matching = filter_workflows_for_branch(workflows.clone(), "feature/foo");
    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].0, "main.yml");

    // Push to "hotfix/urgent" -> both match
    let matching = filter_workflows_for_branch(workflows.clone(), "hotfix/urgent");
    assert_eq!(matching.len(), 2);
}

// =============================================================================
// DAG Validation
// =============================================================================

#[test]
fn test_dag_validation_cycle_detection() {
    let mut jobs = IndexMap::new();
    jobs.insert(
        "a".to_string(),
        JobConfig {
            name: None,
            needs: OneOrMany(vec!["b".to_string()]),
            steps: vec![],
            keep_worktrees: false,
        },
    );
    jobs.insert(
        "b".to_string(),
        JobConfig {
            name: None,
            needs: OneOrMany(vec!["a".to_string()]),
            steps: vec![],
            keep_worktrees: false,
        },
    );

    let result = validate_job_dag(&jobs);
    assert!(result.is_err());
    match result.unwrap_err() {
        DagValidationError::Cycle { involved_jobs } => {
            assert!(involved_jobs.contains(&"a".to_string()));
            assert!(involved_jobs.contains(&"b".to_string()));
        }
        other => panic!("expected Cycle error, got: {}", other),
    }
}

#[test]
fn test_dag_validation_unknown_dependency() {
    let mut jobs = IndexMap::new();
    jobs.insert(
        "deploy".to_string(),
        JobConfig {
            name: None,
            needs: OneOrMany(vec!["nonexistent".to_string()]),
            steps: vec![],
            keep_worktrees: false,
        },
    );

    let result = validate_job_dag(&jobs);
    assert!(result.is_err());
    match result.unwrap_err() {
        DagValidationError::UnknownJob { job, unknown_dep } => {
            assert_eq!(job, "deploy");
            assert_eq!(unknown_dep, "nonexistent");
        }
        other => panic!("expected UnknownJob error, got: {}", other),
    }
}

#[test]
fn test_dag_execution_waves() {
    // Build a diamond DAG: lint + test (parallel) -> deploy
    let mut jobs = IndexMap::new();
    jobs.insert(
        "lint".to_string(),
        JobConfig {
            name: Some("Lint".to_string()),
            needs: OneOrMany(vec![]),
            steps: vec![],
            keep_worktrees: false,
        },
    );
    jobs.insert(
        "test".to_string(),
        JobConfig {
            name: Some("Test".to_string()),
            needs: OneOrMany(vec![]),
            steps: vec![],
            keep_worktrees: false,
        },
    );
    jobs.insert(
        "deploy".to_string(),
        JobConfig {
            name: Some("Deploy".to_string()),
            needs: OneOrMany(vec!["lint".to_string(), "test".to_string()]),
            steps: vec![],
            keep_worktrees: false,
        },
    );

    let waves = validate_job_dag(&jobs).unwrap();
    assert_eq!(waves.len(), 2);
    // Wave 0: lint + test in parallel
    assert_eq!(waves[0].len(), 2);
    assert!(waves[0].contains(&"lint".to_string()));
    assert!(waves[0].contains(&"test".to_string()));
    // Wave 1: deploy
    assert_eq!(waves[1], vec!["deploy"]);
}

// =============================================================================
// Multi-Job Workflows
// =============================================================================

#[test]
fn test_job_failure_cascading_to_dependent_jobs() {
    // Simulates: lint (passed), test (failed) -> deploy should be skipped
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");

    create_test_job(&db, &run_id, "lint", JobStatus::Passed);
    create_test_job(&db, &run_id, "test", JobStatus::Failed);
    create_test_job(&db, &run_id, "deploy", JobStatus::Skipped);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "failed");
}

#[test]
fn test_parallel_jobs_independent_status() {
    // Simulates: lint (running) + test (running) -> both independent
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");

    create_test_job(&db, &run_id, "lint", JobStatus::Passed);
    create_test_job(&db, &run_id, "test", JobStatus::Running);

    let run = db.get_run(&run_id).unwrap().unwrap();
    let jobs = db.get_job_results_for_run(&run_id).unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "running");

    // Now test also passes -> deploy pending
    db.update_job_status(
        &jobs[1].id,
        JobStatus::Passed,
        Some(now_timestamp()),
        Some(now_timestamp()),
        None,
    )
    .unwrap();
    create_test_job(&db, &run_id, "deploy", JobStatus::Pending);

    let run = db.get_run(&run_id).unwrap().unwrap();
    // Still running because deploy is pending
    assert_eq!(db.compute_run_status(&run).unwrap(), "running");
}

#[test]
fn test_approval_in_multi_job_workflow() {
    // Job "lint" passes, job "deploy" awaits approval
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");

    create_test_job(&db, &run_id, "lint", JobStatus::Passed);
    let deploy_job_id = create_test_job(&db, &run_id, "deploy", JobStatus::AwaitingApproval);
    create_step_result(&db, &run_id, &deploy_job_id, "lint", StepStatus::Passed);
    create_step_result(
        &db,
        &run_id,
        &deploy_job_id,
        "push",
        StepStatus::AwaitingApproval,
    );
    create_step_result(
        &db,
        &run_id,
        &deploy_job_id,
        "create-pr",
        StepStatus::Pending,
    );

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "awaiting_approval");

    // Approve the deploy job
    db.update_job_status(
        &deploy_job_id,
        JobStatus::Running,
        Some(now_timestamp()),
        None,
        None,
    )
    .unwrap();

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "running");
}

#[test]
fn test_continue_on_error_at_step_level() {
    // A step with continue-on-error=true fails, but the job continues
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Passed);

    // test step failed (with continue-on-error), but pipeline continued
    create_step_result(&db, &run_id, &job_id, "lint", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Failed); // continue-on-error
    create_step_result(&db, &run_id, &job_id, "push", StepStatus::Passed); // continued

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "completed");
}
