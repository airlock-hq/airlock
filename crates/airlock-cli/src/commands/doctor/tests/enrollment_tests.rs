use super::super::*;
use super::helpers::*;
use airlock_core::Repo;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_check_repo_enrollment_not_git_repo() {
    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().join("airlock"));

    let result = check_repo_enrollment(temp_dir.path(), &paths);
    // Should be a warning (passes but notes it's not a git repo)
    assert!(result.passed);
    assert!(result.message.contains("Not inside a Git repository"));
}

#[test]
fn test_check_repo_enrollment_not_enrolled() {
    let env = TestEnv::setup();

    let result = check_repo_enrollment(&env.working_dir, &env.paths);
    assert!(!result.passed);
    assert!(result.message.contains("not enrolled"));
}

#[test]
fn test_check_repo_enrollment_ok() {
    let env = TestEnv::setup();

    let canonical_path = env.working_dir.canonicalize().unwrap();
    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: canonical_path,
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: PathBuf::from("/tmp/gate.git"),
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };
    env.db.insert_repo(&test_repo).unwrap();

    let result = check_repo_enrollment(&env.working_dir, &env.paths);
    assert!(result.passed);
    assert!(result.message.contains("enrolled"));
}
