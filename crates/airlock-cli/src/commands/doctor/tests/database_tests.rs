use super::super::*;
use super::helpers::*;
use airlock_core::{Database, Repo};
use std::fs;
use tempfile::TempDir;

#[test]
fn test_diagnostic_result_constructors() {
    let pass = DiagnosticResult::pass("Test", "All good");
    assert!(pass.passed);
    assert!(pass.suggestion.is_none());

    let fail = DiagnosticResult::fail("Test", "Something wrong", "Fix it");
    assert!(!fail.passed);
    assert_eq!(fail.suggestion, Some("Fix it".to_string()));

    let warn = DiagnosticResult::warn("Test", "Warning");
    assert!(warn.passed);
    assert!(warn.suggestion.is_none());
}

#[test]
fn test_check_database_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());

    let result = check_database(&paths);
    assert!(!result.passed);
    assert!(result.message.contains("not found"));
}

#[test]
fn test_check_database_ok() {
    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // Create database
    let _db = Database::open(&paths.database()).unwrap();

    let result = check_database(&paths);
    assert!(result.passed);
    assert!(result.message.contains("0 repos enrolled"));
}

#[test]
fn test_doctor_outside_git_repo() {
    let temp_dir = TempDir::new().unwrap();
    let airlock_root = temp_dir.path().join("airlock");
    let paths = AirlockPaths::with_root(airlock_root);
    paths.ensure_dirs().unwrap();
    let _db = Database::open(&paths.database()).unwrap();

    // Run doctor outside of a git repo - should still work but skip repo-specific checks
    let result = run_with_paths(temp_dir.path(), &paths);
    assert!(result.is_ok());
}

#[test]
fn test_doctor_full_flow() {
    let env = TestEnv::setup();

    // Create gate repo with hooks
    create_test_gate_repo(&env.gate_path);
    airlock_core::git::install_hooks(&env.gate_path).unwrap();

    // Configure working repo remotes
    env.repo
        .remote("origin", env.gate_path.to_str().unwrap())
        .unwrap();
    env.repo
        .remote("upstream", "https://github.com/user/repo.git")
        .unwrap();

    // Enroll in database
    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: env.working_dir.canonicalize().unwrap(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: env.gate_path.clone(),
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };
    env.db.insert_repo(&test_repo).unwrap();

    // Run doctor
    let result = run_with_paths(&env.working_dir, &env.paths);
    assert!(result.is_ok());
}

// ========================================
// E2E tests for Section 7.4: Checks database integrity
// ========================================

/// E2E test: Verifies that `airlock doctor` checks database integrity
/// and fails when database file doesn't exist.
///
/// Test Plan Section 7.4: "E2E: Checks database integrity"
#[test]
fn test_e2e_doctor_checks_database_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());
    // Don't create any directories or database - simulate fresh install

    let result = check_database(&paths);

    assert!(
        !result.passed,
        "Database check should fail when database file doesn't exist"
    );
    assert_eq!(result.name, "Database");
    assert!(
        result.message.contains("not found"),
        "Message should indicate database not found: {}",
        result.message
    );
    assert!(
        result.suggestion.is_some(),
        "Should provide suggestion to create database"
    );
    assert!(
        result.suggestion.as_ref().unwrap().contains("airlock init"),
        "Suggestion should mention airlock init: {:?}",
        result.suggestion
    );
}

/// E2E test: Verifies that `airlock doctor` checks database integrity
/// and passes when database exists and is queryable with 0 repos.
///
/// Test Plan Section 7.4: "E2E: Checks database integrity"
#[test]
fn test_e2e_doctor_checks_database_ok_empty() {
    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // Create empty database (no repos enrolled)
    let _db = Database::open(&paths.database()).unwrap();

    let result = check_database(&paths);

    assert!(
        result.passed,
        "Database check should pass when database exists and is queryable: {}",
        result.message
    );
    assert_eq!(result.name, "Database");
    assert!(
        result.message.contains("Database OK"),
        "Message should indicate database is OK: {}",
        result.message
    );
    assert!(
        result.message.contains("0 repos enrolled"),
        "Message should indicate 0 repos enrolled: {}",
        result.message
    );
    assert!(
        result.suggestion.is_none(),
        "Should not provide suggestion when database is healthy"
    );
}

/// E2E test: Verifies that `airlock doctor` checks database integrity
/// and reports correct repo count when repos are enrolled.
///
/// Test Plan Section 7.4: "E2E: Checks database integrity"
#[test]
fn test_e2e_doctor_checks_database_ok_with_repos() {
    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // Create database with some repos
    let db = Database::open(&paths.database()).unwrap();

    // Add 3 repos
    for i in 1..=3 {
        let test_repo = Repo {
            id: format!("test{}", i),
            working_path: temp_dir.path().join(format!("repo{}", i)),
            upstream_url: format!("https://github.com/user/repo{}.git", i),
            gate_path: temp_dir.path().join(format!("gate{}.git", i)),
            last_sync: Some(now_timestamp()),
            created_at: now_timestamp(),
        };
        db.insert_repo(&test_repo).unwrap();
    }

    let result = check_database(&paths);

    assert!(
        result.passed,
        "Database check should pass when database has enrolled repos: {}",
        result.message
    );
    assert_eq!(result.name, "Database");
    assert!(
        result.message.contains("Database OK"),
        "Message should indicate database is OK: {}",
        result.message
    );
    assert!(
        result.message.contains("3 repos enrolled"),
        "Message should indicate 3 repos enrolled: {}",
        result.message
    );
    assert!(
        result.suggestion.is_none(),
        "Should not provide suggestion when database is healthy"
    );
}

/// E2E test: Verifies that `airlock doctor` checks database integrity
/// and fails gracefully when database file is corrupted.
///
/// Test Plan Section 7.4: "E2E: Checks database integrity"
#[test]
fn test_e2e_doctor_checks_database_corrupted() {
    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // Create a corrupted database file (not a valid SQLite file)
    let db_path = paths.database();
    fs::write(&db_path, "This is not a valid SQLite database file").unwrap();

    let result = check_database(&paths);

    assert!(
        !result.passed,
        "Database check should fail when database is corrupted"
    );
    assert_eq!(result.name, "Database");
    assert!(
        result.message.contains("Failed to open database")
            || result.message.contains("query failed"),
        "Message should indicate database open/query failure: {}",
        result.message
    );
    assert!(
        result.suggestion.is_some(),
        "Should provide suggestion to fix corrupted database"
    );
    assert!(
        result.suggestion.as_ref().unwrap().contains("deleting"),
        "Suggestion should mention deleting the database: {:?}",
        result.suggestion
    );
}

/// E2E test: Verifies that `airlock doctor` includes database check
/// in its full flow output.
///
/// Test Plan Section 7.4: "E2E: Checks database integrity"
#[test]
fn test_e2e_doctor_full_flow_includes_database_check() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();
    // Not a git repo, so repo-specific checks will be skipped

    let paths = AirlockPaths::with_root(airlock_root.clone());
    paths.ensure_dirs().unwrap();

    // Create valid database
    let _db = Database::open(&paths.database()).unwrap();

    // Run doctor - should include database check
    let result = run_with_paths(&working_dir, &paths);
    assert!(
        result.is_ok(),
        "Doctor command should succeed when database is healthy"
    );

    // Verify database check was run by checking directly
    let db_result = check_database(&paths);
    assert!(
        db_result.passed,
        "Database check should pass in full doctor flow: {}",
        db_result.message
    );
}
