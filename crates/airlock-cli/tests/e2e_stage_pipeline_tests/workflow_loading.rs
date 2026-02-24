//! Tests for loading workflows from disk, filtering by branch,
//! workflow file tracking, and job/step querying.

use super::helpers::*;
use airlock_core::{
    filter_workflows_for_branch, load_workflows_from_disk, JobStatus, PushTrigger, Run, StepStatus,
    TriggerConfig, WorkflowConfig,
};
use indexmap::IndexMap;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_load_workflows_from_disk() {
    let temp_dir = TempDir::new().unwrap();
    let repo_root = temp_dir.path();

    // Create .airlock/workflows/ with two workflow files
    let workflows_dir = repo_root.join(".airlock/workflows");
    fs::create_dir_all(&workflows_dir).unwrap();

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
  quick:
    steps:
      - name: push
        run: echo push
"#;

    fs::write(workflows_dir.join("main.yml"), main_yaml).unwrap();
    fs::write(workflows_dir.join("hotfix.yaml"), hotfix_yaml).unwrap();

    let workflows = load_workflows_from_disk(repo_root).unwrap();
    assert_eq!(workflows.len(), 2);

    // Verify filenames
    let filenames: Vec<&str> = workflows.iter().map(|(f, _)| f.as_str()).collect();
    assert!(filenames.contains(&"main.yml"));
    assert!(filenames.contains(&"hotfix.yaml"));

    // Verify content
    for (filename, config) in &workflows {
        if filename == "main.yml" {
            assert_eq!(config.name, Some("Main".to_string()));
            assert_eq!(config.jobs.len(), 1);
        } else if filename == "hotfix.yaml" {
            assert_eq!(config.name, Some("Hotfix".to_string()));
            assert!(config.jobs.contains_key("quick"));
        }
    }
}

#[test]
fn test_filter_workflows_for_branch_multiple_matches() {
    let main_config = WorkflowConfig {
        name: Some("Main".to_string()),
        on: Some(TriggerConfig {
            push: Some(PushTrigger {
                branches: vec!["**".to_string()],
                branches_ignore: vec![],
            }),
        }),
        jobs: IndexMap::new(),
    };

    let hotfix_config = WorkflowConfig {
        name: Some("Hotfix".to_string()),
        on: Some(TriggerConfig {
            push: Some(PushTrigger {
                branches: vec!["hotfix/**".to_string()],
                branches_ignore: vec![],
            }),
        }),
        jobs: IndexMap::new(),
    };

    let workflows = vec![
        ("main.yml".to_string(), main_config),
        ("hotfix.yml".to_string(), hotfix_config),
    ];

    // feature/foo -> only main
    let matching = filter_workflows_for_branch(workflows.clone(), "feature/foo");
    assert_eq!(matching.len(), 1);
    assert_eq!(matching[0].0, "main.yml");

    // hotfix/urgent -> both
    let matching = filter_workflows_for_branch(workflows, "hotfix/urgent");
    assert_eq!(matching.len(), 2);
}

#[test]
fn test_workflow_run_tracks_workflow_file() {
    // Verify that runs store which workflow file triggered them
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = format!("run_{}", uuid::Uuid::new_v4());
    let created = now_timestamp();
    let run = Run {
        id: run_id.clone(),
        repo_id: repo_id.to_string(),
        ref_updates: vec![],
        error: None,
        superseded: false,
        created_at: created,
        branch: "refs/heads/main".to_string(),
        base_sha: "abc123".to_string(),
        head_sha: "def456".to_string(),
        current_step: None,
        workflow_file: "hotfix.yml".to_string(),
        workflow_name: Some("Hotfix Pipeline".to_string()),
        updated_at: created,
    };
    db.insert_run(&run).unwrap();

    let retrieved = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(retrieved.workflow_file, "hotfix.yml");
    assert_eq!(retrieved.workflow_name, Some("Hotfix Pipeline".to_string()));
}

#[test]
fn test_step_results_for_specific_job() {
    // Verify we can query step results scoped to a specific job
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");

    let lint_job_id = create_test_job(&db, &run_id, "lint", JobStatus::Passed);
    create_step_result(&db, &run_id, &lint_job_id, "eslint", StepStatus::Passed);
    create_step_result(&db, &run_id, &lint_job_id, "prettier", StepStatus::Passed);

    let test_job_id = create_test_job(&db, &run_id, "test", JobStatus::Running);
    create_step_result(&db, &run_id, &test_job_id, "unit-tests", StepStatus::Passed);
    create_step_result(
        &db,
        &run_id,
        &test_job_id,
        "integration-tests",
        StepStatus::Running,
    );

    // All steps for the run
    let all_steps = db.get_step_results_for_run(&run_id).unwrap();
    assert_eq!(all_steps.len(), 4);

    // Steps for lint job only
    let lint_steps = db.get_step_results_for_job(&lint_job_id).unwrap();
    assert_eq!(lint_steps.len(), 2);
    assert!(lint_steps.iter().all(|s| s.job_id == lint_job_id));

    // Steps for test job only
    let test_steps = db.get_step_results_for_job(&test_job_id).unwrap();
    assert_eq!(test_steps.len(), 2);
    assert!(test_steps.iter().all(|s| s.job_id == test_job_id));
}

#[test]
fn test_job_status_update() {
    let (_temp_dir, _paths, _working_path, repo_id, db) = setup_airlock_env();

    let run_id = create_test_run(&db, &repo_id, "refs/heads/main");
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Pending);

    // Verify initial state
    let job = db.get_job_result(&job_id).unwrap().unwrap();
    assert_eq!(job.status, JobStatus::Pending);
    assert!(job.started_at.is_none());

    // Update to Running
    let now = now_timestamp();
    db.update_job_status(&job_id, JobStatus::Running, Some(now), None, None)
        .unwrap();

    let job = db.get_job_result(&job_id).unwrap().unwrap();
    assert_eq!(job.status, JobStatus::Running);
    assert_eq!(job.started_at, Some(now));
    assert!(job.completed_at.is_none());

    // Update to Passed
    let completed = now_timestamp();
    db.update_job_status(&job_id, JobStatus::Passed, None, Some(completed), None)
        .unwrap();

    let job = db.get_job_result(&job_id).unwrap().unwrap();
    assert_eq!(job.status, JobStatus::Passed);
    assert_eq!(job.started_at, Some(now)); // Preserved from before
    assert_eq!(job.completed_at, Some(completed));
}
