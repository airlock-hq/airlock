//! Tests for exec commands.

use super::env::ExecEnvironment;
use super::helpers;
use super::JsonArgs;
use serial_test::serial;
use std::env;
use tempfile::TempDir;

/// Set up the environment variables for testing.
fn setup_test_env(temp_dir: &TempDir) -> std::collections::HashMap<String, String> {
    let worktree = temp_dir.path().join("worktree");
    let artifacts = temp_dir.path().join("artifacts");
    let run_artifacts = temp_dir.path().join("run_artifacts");
    let repo_root = temp_dir.path().join("repo");

    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::create_dir_all(&artifacts).unwrap();
    std::fs::create_dir_all(&run_artifacts).unwrap();
    std::fs::create_dir_all(&repo_root).unwrap();

    let mut vars = std::collections::HashMap::new();
    vars.insert("AIRLOCK_RUN_ID".to_string(), "run-test-123".to_string());
    vars.insert(
        "AIRLOCK_BRANCH".to_string(),
        "refs/heads/feature/test".to_string(),
    );
    vars.insert("AIRLOCK_BASE_SHA".to_string(), "abc123".to_string());
    vars.insert("AIRLOCK_HEAD_SHA".to_string(), "def456".to_string());
    vars.insert(
        "AIRLOCK_WORKTREE".to_string(),
        worktree.to_string_lossy().to_string(),
    );
    vars.insert(
        "AIRLOCK_ARTIFACTS".to_string(),
        artifacts.to_string_lossy().to_string(),
    );
    vars.insert(
        "AIRLOCK_RUN_ARTIFACTS".to_string(),
        run_artifacts.to_string_lossy().to_string(),
    );
    vars.insert(
        "AIRLOCK_REPO_ROOT".to_string(),
        repo_root.to_string_lossy().to_string(),
    );
    vars.insert(
        "AIRLOCK_UPSTREAM_URL".to_string(),
        "git@github.com:user/repo.git".to_string(),
    );

    // Set environment variables
    for (key, value) in &vars {
        env::set_var(key, value);
    }

    vars
}

/// Clear the environment variables after testing.
fn clear_test_env() {
    let keys = [
        "AIRLOCK_RUN_ID",
        "AIRLOCK_BRANCH",
        "AIRLOCK_BASE_SHA",
        "AIRLOCK_HEAD_SHA",
        "AIRLOCK_WORKTREE",
        "AIRLOCK_ARTIFACTS",
        "AIRLOCK_RUN_ARTIFACTS",
        "AIRLOCK_REPO_ROOT",
        "AIRLOCK_UPSTREAM_URL",
    ];
    for key in &keys {
        env::remove_var(key);
    }
}

#[test]
#[serial]
fn test_exec_environment_from_env() {
    let temp_dir = TempDir::new().unwrap();
    let _vars = setup_test_env(&temp_dir);

    let exec_env = ExecEnvironment::from_env().unwrap();

    assert_eq!(exec_env.run_id, "run-test-123");
    assert_eq!(exec_env.branch, "refs/heads/feature/test");
    assert_eq!(exec_env.base_sha, "abc123");
    assert_eq!(exec_env.head_sha, "def456");
    assert!(exec_env.worktree.exists());
    assert!(exec_env.artifacts.exists());
    assert!(exec_env.repo_root.exists());
    assert_eq!(exec_env.upstream_url, "git@github.com:user/repo.git");

    clear_test_env();
}

#[test]
#[serial]
fn test_exec_environment_missing_run_id() {
    clear_test_env();

    let result = ExecEnvironment::from_env();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("AIRLOCK_RUN_ID not set"));
}

#[test]
#[serial]
fn test_exec_environment_missing_branch() {
    clear_test_env();
    env::set_var("AIRLOCK_RUN_ID", "test-123");

    let result = ExecEnvironment::from_env();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("AIRLOCK_BRANCH not set"));

    clear_test_env();
}

#[test]
#[serial]
fn test_exec_environment_missing_base_sha() {
    clear_test_env();
    env::set_var("AIRLOCK_RUN_ID", "test-123");
    env::set_var("AIRLOCK_BRANCH", "main");

    let result = ExecEnvironment::from_env();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("AIRLOCK_BASE_SHA not set"));

    clear_test_env();
}

// =========================================================================
// JSON helper tests
// =========================================================================

#[test]
fn test_json_args_creation() {
    let args = JsonArgs {
        path: "title".to_string(),
        set_fields: vec![],
    };
    assert_eq!(args.path, "title");
    assert!(args.set_fields.is_empty());
}

#[test]
fn test_json_args_with_set_fields() {
    let args = JsonArgs {
        path: ".".to_string(),
        set_fields: vec!["key1=value1".to_string(), "key2=123".to_string()],
    };
    assert_eq!(args.path, ".");
    assert_eq!(args.set_fields.len(), 2);
}

// =========================================================================
// Helper tests
// =========================================================================

#[test]
fn test_extract_branch_name_with_refs_prefix() {
    let branch = "refs/heads/feature/new-feature";
    let result = helpers::extract_branch_name(branch);
    assert_eq!(result, "feature/new-feature");
}

#[test]
fn test_extract_branch_name_without_prefix() {
    let branch = "feature/new-feature";
    let result = helpers::extract_branch_name(branch);
    assert_eq!(result, "feature/new-feature");
}

#[test]
fn test_extract_branch_name_main() {
    let branch = "refs/heads/main";
    let result = helpers::extract_branch_name(branch);
    assert_eq!(result, "main");
}
