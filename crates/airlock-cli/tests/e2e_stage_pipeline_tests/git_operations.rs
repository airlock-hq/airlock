//! Tests for real git push flow, diffs, remotes, fetch-through,
//! upload-pack, workflow config from commits, and repoint tracking.

use super::helpers::*;
use airlock_core::{
    compute_diff, git, parse_workflow_config, show_file, DiffResult, JobStatus, Run, StepStatus,
    WorkflowConfig,
};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

// =============================================================================
// Local Git Helpers
// =============================================================================

/// Helper to execute git commands and return output
fn git_command(args: &[&str], cwd: &Path) -> std::process::Output {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "Test User")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test User")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .expect("Failed to execute git command")
}

/// Helper to get the HEAD commit SHA
fn get_head_sha(repo_path: &Path) -> String {
    let output = git_command(&["rev-parse", "HEAD"], repo_path);
    assert!(output.status.success(), "Failed to get HEAD SHA");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Helper to create a commit with a file change
fn create_commit(repo_path: &Path, filename: &str, content: &str, message: &str) -> String {
    let file_path = repo_path.join(filename);
    fs::write(&file_path, content).expect("Failed to write file");

    let output = git_command(&["add", filename], repo_path);
    assert!(output.status.success(), "Failed to stage file");

    let output = git_command(&["commit", "-m", message], repo_path);
    assert!(output.status.success(), "Failed to commit");

    get_head_sha(repo_path)
}

/// Helper to create and checkout a new branch
fn create_branch(repo_path: &Path, branch_name: &str) {
    let output = git_command(&["checkout", "-b", branch_name], repo_path);
    assert!(
        output.status.success(),
        "Failed to create branch: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Create a run in the database with real SHAs from actual commits
fn create_run_with_real_shas(
    db: &airlock_core::Database,
    repo_id: &str,
    branch: &str,
    base_sha: &str,
    head_sha: &str,
) -> String {
    let run_id = format!("run_{}", uuid::Uuid::new_v4());
    let created = now_timestamp();
    let run = Run {
        id: run_id.clone(),
        repo_id: repo_id.to_string(),
        ref_updates: vec![],
        error: None,
        superseded: false,
        created_at: created,
        branch: branch.to_string(),
        base_sha: base_sha.to_string(),
        head_sha: head_sha.to_string(),
        current_step: None,
        workflow_file: "main.yml".to_string(),
        workflow_name: Some("Main Pipeline".to_string()),
        updated_at: created,
    };
    db.insert_run(&run).unwrap();
    run_id
}

// =============================================================================
// Real Git Push Flow
// =============================================================================

#[test]
fn test_diff_computation_for_new_branch() {
    let (temp_dir, _paths, working_path, repo_id, db) = setup_airlock_env();

    let repo = db.get_repo(&repo_id).unwrap().unwrap();
    let gate_path = &repo.gate_path;

    let output = git_command(&["push", "-u", "origin", "master"], &working_path);
    assert!(
        output.status.success(),
        "Initial push failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    create_branch(&working_path, "feature/new-feature");

    let head_sha = create_commit(
        &working_path,
        "new_feature.rs",
        "// New feature implementation\npub fn new_feature() { println!(\"Hello!\"); }\n",
        "Add new feature",
    );

    let null_sha = "0000000000000000000000000000000000000000";

    let _run_id = create_run_with_real_shas(
        &db,
        &repo_id,
        "refs/heads/feature/new-feature",
        null_sha,
        &head_sha,
    );

    let output = git_command(
        &["push", "-u", "origin", "feature/new-feature"],
        &working_path,
    );
    assert!(
        output.status.success(),
        "Feature branch push failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let diff_result: DiffResult = compute_diff(gate_path, null_sha, &head_sha);

    assert!(!diff_result.patch.is_empty());
    assert!(!diff_result.files_changed.is_empty());
    assert!(diff_result
        .files_changed
        .contains(&"new_feature.rs".to_string()));
    assert!(diff_result.additions > 0);
    assert!(diff_result.patch.contains("new_feature.rs"));
    assert!(diff_result.patch.contains("+// New feature implementation"));

    drop(temp_dir);
}

#[test]
fn test_diff_computation_for_branch_update() {
    let (temp_dir, _paths, working_path, repo_id, db) = setup_airlock_env();

    let repo = db.get_repo(&repo_id).unwrap().unwrap();
    let gate_path = &repo.gate_path;

    let output = git_command(&["push", "-u", "origin", "master"], &working_path);
    assert!(output.status.success(), "Initial push failed");

    let base_sha = get_head_sha(&working_path);

    let head_sha = create_commit(
        &working_path,
        "updated_file.rs",
        "// Updated content\npub fn updated() {}\n",
        "Update with new file",
    );

    let _run_id =
        create_run_with_real_shas(&db, &repo_id, "refs/heads/master", &base_sha, &head_sha);

    let output = git_command(&["push", "origin", "master"], &working_path);
    assert!(output.status.success(), "Update push failed");

    let diff_result = compute_diff(gate_path, &base_sha, &head_sha);

    assert!(!diff_result.patch.is_empty());
    assert!(diff_result
        .files_changed
        .contains(&"updated_file.rs".to_string()));
    assert!(diff_result.additions > 0);
    assert_eq!(diff_result.effective_base_sha, base_sha);

    drop(temp_dir);
}

#[test]
fn test_run_with_real_shas_and_steps() {
    let (temp_dir, _paths, working_path, repo_id, db) = setup_airlock_env();

    let repo = db.get_repo(&repo_id).unwrap().unwrap();
    let gate_path = &repo.gate_path;

    let output = git_command(&["push", "-u", "origin", "master"], &working_path);
    assert!(output.status.success());

    let _master_sha = get_head_sha(&working_path);

    create_branch(&working_path, "feature/with-steps");
    let head_sha = create_commit(
        &working_path,
        "feature_code.rs",
        "pub fn feature() { /* implementation */ }\n",
        "Implement feature",
    );

    let output = git_command(
        &["push", "-u", "origin", "feature/with-steps"],
        &working_path,
    );
    assert!(output.status.success());

    let merge_base_output = git_command(&["merge-base", "master", &head_sha], &working_path);
    let expected_merge_base = String::from_utf8_lossy(&merge_base_output.stdout)
        .trim()
        .to_string();

    let null_sha = "0000000000000000000000000000000000000000";
    let run_id = create_run_with_real_shas(
        &db,
        &repo_id,
        "refs/heads/feature/with-steps",
        null_sha,
        &head_sha,
    );

    // Add job and step results (simulating pipeline execution)
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::AwaitingApproval);
    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "test", StepStatus::Passed);
    create_step_result(&db, &run_id, &job_id, "push", StepStatus::AwaitingApproval);

    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "awaiting_approval");

    let diff_result = compute_diff(gate_path, null_sha, &head_sha);

    assert_eq!(diff_result.effective_base_sha, expected_merge_base);
    assert!(diff_result
        .files_changed
        .contains(&"feature_code.rs".to_string()));

    drop(temp_dir);
}

#[test]
fn test_multiple_commits_on_branch_diff() {
    let (temp_dir, _paths, working_path, repo_id, db) = setup_airlock_env();

    let repo = db.get_repo(&repo_id).unwrap().unwrap();
    let gate_path = &repo.gate_path;

    let output = git_command(&["push", "-u", "origin", "master"], &working_path);
    assert!(output.status.success());

    let base_sha = get_head_sha(&working_path);

    create_branch(&working_path, "feature/multi-commit");

    create_commit(
        &working_path,
        "file1.rs",
        "// First file\npub fn one() {}\n",
        "Add first file",
    );
    create_commit(
        &working_path,
        "file2.rs",
        "// Second file\npub fn two() {}\n",
        "Add second file",
    );
    let head_sha = create_commit(
        &working_path,
        "file3.rs",
        "// Third file\npub fn three() {}\n",
        "Add third file",
    );

    let output = git_command(
        &["push", "-u", "origin", "feature/multi-commit"],
        &working_path,
    );
    assert!(output.status.success());

    let diff_result = compute_diff(gate_path, &base_sha, &head_sha);

    assert_eq!(diff_result.files_changed.len(), 3);
    assert!(diff_result.files_changed.contains(&"file1.rs".to_string()));
    assert!(diff_result.files_changed.contains(&"file2.rs".to_string()));
    assert!(diff_result.files_changed.contains(&"file3.rs".to_string()));

    assert!(diff_result.patch.contains("First file"));
    assert!(diff_result.patch.contains("Second file"));
    assert!(diff_result.patch.contains("Third file"));

    drop(temp_dir);
}

#[test]
fn test_init_creates_correct_remote_structure() {
    let (temp_dir, _paths, working_path, repo_id, db) = setup_airlock_env();

    let repo = db.get_repo(&repo_id).unwrap().unwrap();

    let origin_output = git_command(&["remote", "get-url", "origin"], &working_path);
    assert!(origin_output.status.success());
    let origin_url = String::from_utf8_lossy(&origin_output.stdout)
        .trim()
        .to_string();

    let upstream_output = git_command(&["remote", "get-url", "upstream"], &working_path);
    assert!(upstream_output.status.success());
    let upstream_url = String::from_utf8_lossy(&upstream_output.stdout)
        .trim()
        .to_string();

    assert!(origin_url.contains(".git") || Path::new(&origin_url).exists());
    assert_eq!(upstream_url, repo.upstream_url);
    assert!(
        origin_url.contains(&repo.gate_path.to_string_lossy().to_string())
            || repo.gate_path.to_string_lossy().contains(&origin_url)
    );

    drop(temp_dir);
}

#[test]
fn test_escape_hatch_push_to_upstream() {
    let (temp_dir, _paths, working_path, _repo_id, _db) = setup_airlock_env();

    let output = git_command(&["push", "-u", "origin", "master"], &working_path);
    assert!(output.status.success());

    create_commit(
        &working_path,
        "escape_hatch_test.rs",
        "// Escape hatch test\n",
        "Test escape hatch",
    );

    let output = git_command(&["push", "upstream", "master"], &working_path);
    assert!(
        output.status.success(),
        "Push to upstream (escape hatch) should work: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    drop(temp_dir);
}

#[test]
fn test_diff_with_file_modifications_deletions_and_additions() {
    let (temp_dir, _paths, working_path, repo_id, db) = setup_airlock_env();

    let repo = db.get_repo(&repo_id).unwrap().unwrap();
    let gate_path = &repo.gate_path;

    create_commit(
        &working_path,
        "to_modify.rs",
        "// Original content\npub fn original() {}\n",
        "Add file to modify",
    );
    create_commit(
        &working_path,
        "to_delete.rs",
        "// This will be deleted\n",
        "Add file to delete",
    );

    let output = git_command(&["push", "-u", "origin", "master"], &working_path);
    assert!(output.status.success());

    let base_sha = get_head_sha(&working_path);

    create_branch(&working_path, "feature/mixed-changes");

    fs::write(
        working_path.join("to_modify.rs"),
        "// Modified content\npub fn modified() { /* changed */ }\n",
    )
    .unwrap();
    git_command(&["add", "to_modify.rs"], &working_path);
    git_command(&["commit", "-m", "Modify file"], &working_path);

    fs::remove_file(working_path.join("to_delete.rs")).unwrap();
    git_command(&["add", "to_delete.rs"], &working_path);
    git_command(&["commit", "-m", "Delete file"], &working_path);

    let head_sha = create_commit(
        &working_path,
        "new_file.rs",
        "// Brand new file\npub fn new_func() {}\n",
        "Add new file",
    );

    let output = git_command(
        &["push", "-u", "origin", "feature/mixed-changes"],
        &working_path,
    );
    assert!(output.status.success());

    let diff_result = compute_diff(gate_path, &base_sha, &head_sha);

    assert!(diff_result
        .files_changed
        .contains(&"to_modify.rs".to_string()));
    assert!(diff_result
        .files_changed
        .contains(&"to_delete.rs".to_string()));
    assert!(diff_result
        .files_changed
        .contains(&"new_file.rs".to_string()));
    assert!(diff_result.additions > 0);
    assert!(diff_result.deletions > 0);
    assert!(diff_result.patch.contains("--- a/to_delete.rs"));
    assert!(diff_result.patch.contains("+++ b/new_file.rs"));

    drop(temp_dir);
}

// =============================================================================
// Run State Transitions with Real Repo
// =============================================================================

#[test]
fn test_run_state_transitions_with_real_repo() {
    let (temp_dir, _paths, working_path, repo_id, db) = setup_airlock_env();

    let output = git_command(&["push", "-u", "origin", "master"], &working_path);
    assert!(output.status.success());

    let base_sha = get_head_sha(&working_path);

    create_branch(&working_path, "feature/state-test");
    let head_sha = create_commit(
        &working_path,
        "state_test.rs",
        "pub fn test() {}\n",
        "Test commit",
    );

    let output = git_command(
        &["push", "-u", "origin", "feature/state-test"],
        &working_path,
    );
    assert!(output.status.success());

    let run_id = create_run_with_real_shas(
        &db,
        &repo_id,
        "refs/heads/feature/state-test",
        &base_sha,
        &head_sha,
    );

    // No steps yet -> pending
    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "pending");

    // Add running step -> running
    let job_id = create_test_job(&db, &run_id, "default", JobStatus::Running);
    create_step_result(&db, &run_id, &job_id, "describe", StepStatus::Running);
    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "running");

    // Complete describe, add awaiting approval -> awaiting_approval
    let steps = db.get_step_results_for_run(&run_id).unwrap();
    let describe_step = steps.iter().find(|s| s.name == "describe").unwrap();
    let mut updated = describe_step.clone();
    updated.status = StepStatus::Passed;
    db.update_step_result(&updated).unwrap();

    create_step_result(&db, &run_id, &job_id, "push", StepStatus::AwaitingApproval);
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

    // Approve push -> continue to completion
    let steps = db.get_step_results_for_run(&run_id).unwrap();
    let push_step = steps.iter().find(|s| s.name == "push").unwrap();
    let mut updated = push_step.clone();
    updated.status = StepStatus::Passed;
    db.update_step_result(&updated).unwrap();

    create_step_result(&db, &run_id, &job_id, "create-pr", StepStatus::Passed);
    // Update job to passed
    db.update_job_status(
        &job_id,
        JobStatus::Passed,
        Some(now_timestamp()),
        Some(now_timestamp()),
        None,
    )
    .unwrap();
    let run = db.get_run(&run_id).unwrap().unwrap();
    assert_eq!(db.compute_run_status(&run).unwrap(), "completed");

    drop(temp_dir);
}

// =============================================================================
// Gate Fetch-Through and Upload-Pack Wrapper
// =============================================================================

#[test]
fn test_fetch_through_gate_reflects_upstream_updates() {
    let (_temp_dir, _paths, working_path, repo_id, db) = setup_airlock_env();

    let repo = db.get_repo(&repo_id).unwrap().unwrap();
    let gate_path = &repo.gate_path;

    let output = git_command(&["push", "-u", "origin", "master"], &working_path);
    assert!(output.status.success());

    git::mirror_from_remote(gate_path, "origin").expect("First mirror should succeed");

    let upstream_path_output = git_command(&["remote", "get-url", "origin"], gate_path);
    let upstream_path = String::from_utf8_lossy(&upstream_path_output.stdout)
        .trim()
        .to_string();

    let upstream_clone_dir = _temp_dir.path().join("upstream_clone");
    let clone_output = Command::new("git")
        .args([
            "clone",
            &upstream_path,
            upstream_clone_dir.to_str().unwrap(),
        ])
        .env("GIT_AUTHOR_NAME", "Other Dev")
        .env("GIT_AUTHOR_EMAIL", "other@example.com")
        .env("GIT_COMMITTER_NAME", "Other Dev")
        .env("GIT_COMMITTER_EMAIL", "other@example.com")
        .output()
        .expect("Failed to clone upstream");
    assert!(clone_output.status.success());

    let new_sha = create_commit(
        &upstream_clone_dir,
        "upstream_change.txt",
        "This was pushed directly to upstream\n",
        "Upstream commit by another developer",
    );

    let push_output = git_command(&["push", "origin", "master"], &upstream_clone_dir);
    assert!(push_output.status.success());

    git::mirror_from_remote(gate_path, "origin").expect("Second mirror should succeed");

    let fetch_output = git_command(&["fetch", "origin"], &working_path);
    assert!(fetch_output.status.success());

    let log_output = git_command(
        &["log", "--oneline", "-1", "refs/remotes/origin/master"],
        &working_path,
    );
    assert!(log_output.status.success());
    let log_str = String::from_utf8_lossy(&log_output.stdout);
    assert!(log_str.contains("Upstream commit by another developer"));

    let rev_output = git_command(&["rev-parse", "refs/remotes/origin/master"], &working_path);
    let fetched_sha = String::from_utf8_lossy(&rev_output.stdout)
        .trim()
        .to_string();
    assert_eq!(fetched_sha, new_sha);
}

#[test]
fn test_init_configures_upload_pack_wrapper() {
    let (_temp_dir, paths, working_path, _repo_id, _db) = setup_airlock_env();

    git::install_upload_pack_wrapper(&paths).expect("Should install wrapper");

    let wrapper_path = paths.upload_pack_wrapper();
    assert!(wrapper_path.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::metadata(&wrapper_path).unwrap().permissions();
        assert!(perms.mode() & 0o111 != 0, "Wrapper should be executable");
    }

    let content = fs::read_to_string(&wrapper_path).unwrap();
    assert!(content.contains("exec git-upload-pack"));
    assert!(content.contains("fetch_notification"));
    assert!(content.contains("nc -U"));

    git::configure_upload_pack(&working_path, &wrapper_path).expect("Should configure upload-pack");

    let config_output = git_command(&["config", "remote.origin.uploadpack"], &working_path);
    assert!(config_output.status.success());
    let configured_path = String::from_utf8_lossy(&config_output.stdout)
        .trim()
        .to_string();
    assert_eq!(configured_path, wrapper_path.to_string_lossy());
}

// =============================================================================
// Reading Workflow Config from Pushed Commit
// =============================================================================

#[test]
fn test_show_file_reads_workflow_from_pushed_commit_not_working_dir() {
    let (_temp_dir, _paths, working_path, repo_id, db) = setup_airlock_env();

    let repo = db.get_repo(&repo_id).unwrap().unwrap();
    let gate_path = &repo.gate_path;

    // Create .airlock/workflows/main.yml with echo A
    let config_a = r#"name: Pipeline A

on:
  push:
    branches: ['**']

jobs:
  default:
    steps:
      - name: test
        run: echo A
"#;
    fs::create_dir_all(working_path.join(".airlock/workflows")).unwrap();
    fs::write(working_path.join(".airlock/workflows/main.yml"), config_a).unwrap();
    git_command(&["add", ".airlock/workflows/main.yml"], &working_path);
    git_command(
        &[
            "commit",
            "-m",
            "Add .airlock/workflows/main.yml with echo A",
        ],
        &working_path,
    );

    let output = git_command(&["push", "-u", "origin", "master"], &working_path);
    assert!(
        output.status.success(),
        "Push failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let pushed_sha = get_head_sha(&working_path);

    // Modify on disk WITHOUT pushing
    let config_b = r#"name: Pipeline B

on:
  push:
    branches: ['**']

jobs:
  default:
    steps:
      - name: test
        run: echo B
"#;
    fs::write(working_path.join(".airlock/workflows/main.yml"), config_b).unwrap();

    let disk_content =
        fs::read_to_string(working_path.join(".airlock/workflows/main.yml")).unwrap();
    assert!(disk_content.contains("echo B"));

    // show_file should return the PUSHED version
    let gate_content = show_file(gate_path, &pushed_sha, ".airlock/workflows/main.yml").unwrap();
    assert!(
        gate_content.contains("echo A"),
        "show_file should return pushed version (echo A), got: {}",
        gate_content
    );
    assert!(
        !gate_content.contains("echo B"),
        "show_file should NOT return working directory version (echo B)"
    );

    // Verify it parses as a valid WorkflowConfig
    let config: WorkflowConfig = parse_workflow_config(&gate_content).unwrap();
    assert_eq!(config.name, Some("Pipeline A".to_string()));
}

#[test]
fn test_show_file_returns_different_workflows_for_different_branches() {
    let (_temp_dir, _paths, working_path, repo_id, db) = setup_airlock_env();

    let repo = db.get_repo(&repo_id).unwrap().unwrap();
    let gate_path = &repo.gate_path;

    // Create workflow on master
    let config_master = r#"name: Master Pipeline

on:
  push:
    branches: ['**']

jobs:
  default:
    steps:
      - name: test
        run: echo master
"#;
    fs::create_dir_all(working_path.join(".airlock/workflows")).unwrap();
    fs::write(
        working_path.join(".airlock/workflows/main.yml"),
        config_master,
    )
    .unwrap();
    git_command(&["add", ".airlock/workflows/main.yml"], &working_path);
    git_command(&["commit", "-m", "Add workflow for master"], &working_path);

    let output = git_command(&["push", "-u", "origin", "master"], &working_path);
    assert!(output.status.success());
    let master_sha = get_head_sha(&working_path);

    // Create a feature branch with a different workflow
    create_branch(&working_path, "feature/custom-pipeline");
    let config_feature = r#"name: Feature Pipeline

on:
  push:
    branches: ['feature/**']

jobs:
  default:
    steps:
      - name: lint
        run: echo feature-lint
      - name: test
        run: echo feature-test
"#;
    fs::write(
        working_path.join(".airlock/workflows/main.yml"),
        config_feature,
    )
    .unwrap();
    git_command(&["add", ".airlock/workflows/main.yml"], &working_path);
    git_command(
        &["commit", "-m", "Update workflow for feature branch"],
        &working_path,
    );

    let output = git_command(
        &["push", "-u", "origin", "feature/custom-pipeline"],
        &working_path,
    );
    assert!(output.status.success());
    let feature_sha = get_head_sha(&working_path);

    // Read config from master's commit
    let master_content = show_file(gate_path, &master_sha, ".airlock/workflows/main.yml").unwrap();
    assert!(master_content.contains("echo master"));

    // Read config from feature branch's commit
    let feature_content =
        show_file(gate_path, &feature_sha, ".airlock/workflows/main.yml").unwrap();
    assert!(feature_content.contains("echo feature-lint"));
    assert!(feature_content.contains("echo feature-test"));

    // Verify the configs are actually different
    assert_ne!(master_content, feature_content);

    // Verify both parse as valid WorkflowConfigs
    let master_config: WorkflowConfig = parse_workflow_config(&master_content).unwrap();
    assert_eq!(master_config.name, Some("Master Pipeline".to_string()));

    let feature_config: WorkflowConfig = parse_workflow_config(&feature_content).unwrap();
    assert_eq!(feature_config.name, Some("Feature Pipeline".to_string()));
    assert_eq!(feature_config.jobs["default"].steps.len(), 2);
}

// =============================================================================
// Repoint Tracking Branches
// =============================================================================

#[test]
fn test_repoint_tracking_branches_after_init() {
    let temp_dir = TempDir::new().unwrap();
    let upstream_dir = temp_dir.path().join("upstream.git");
    let working_dir = temp_dir.path().join("working");
    let gate_dir = temp_dir.path().join("gate.git");

    fs::create_dir_all(&working_dir).unwrap();

    create_upstream_repo(&upstream_dir);
    let upstream_url = upstream_dir.to_string_lossy().to_string();
    create_working_repo(&working_dir, &upstream_url);
    let push_output = git_command(&["push", "-u", "origin", "master"], &working_dir);
    assert!(push_output.status.success());

    let working_repo = git::discover_repo(&working_dir).unwrap();
    git::rename_remote(&working_repo, "origin", "upstream").unwrap();

    let tracking_output = git_command(
        &[
            "for-each-ref",
            "--format=%(upstream:remotename)",
            "refs/heads/master",
        ],
        &working_dir,
    );
    assert_eq!(
        String::from_utf8_lossy(&tracking_output.stdout).trim(),
        "upstream"
    );

    let gate_repo = git::create_bare_repo(&gate_dir).unwrap();
    git::add_remote(&gate_repo, "origin", &upstream_url).unwrap();
    let gate_url = gate_dir.to_string_lossy().to_string();
    git::add_remote(&working_repo, "origin", &gate_url).unwrap();

    git::mirror_from_remote(&gate_dir, "origin").expect("Mirror should succeed");
    git::fetch(&working_dir, "origin").expect("Fetch from gate should succeed");
    git::repoint_tracking_branches(&working_dir, "upstream", "origin")
        .expect("Repoint should succeed");

    let tracking_output = git_command(
        &["for-each-ref", "--format=%(upstream)", "refs/heads/master"],
        &working_dir,
    );
    assert_eq!(
        String::from_utf8_lossy(&tracking_output.stdout).trim(),
        "refs/remotes/origin/master"
    );
}
