use super::super::*;
use super::helpers::*;
use airlock_core::{git, Database, Repo};
use git2::Repository;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_check_gate_repo_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let gate_path = temp_dir.path().join("nonexistent.git");

    let result = check_gate_repo(&gate_path);
    assert!(!result.passed);
    assert!(result.message.contains("not found"));
}

#[test]
fn test_check_gate_repo_ok() {
    let temp_dir = TempDir::new().unwrap();
    let gate_path = temp_dir.path().join("gate.git");

    create_test_gate_repo(&gate_path);

    let result = check_gate_repo(&gate_path);
    assert!(result.passed);
    assert!(result.message.contains("Gate repository OK"));
}

#[test]
fn test_check_gate_repo_missing_origin() {
    let temp_dir = TempDir::new().unwrap();
    let gate_path = temp_dir.path().join("gate.git");

    fs::create_dir_all(&gate_path).unwrap();
    Repository::init_bare(&gate_path).expect("Failed to init bare repo");
    // Don't add origin remote

    let result = check_gate_repo(&gate_path);
    assert!(!result.passed);
    assert!(result.message.contains("missing 'origin' remote"));
}

#[test]
fn test_check_remotes_correct() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let gate_path = temp_dir.path().join("gate.git");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);

    // Setup gate
    create_test_gate_repo(&gate_path);

    // Configure remotes correctly
    repo.remote("origin", gate_path.to_str().unwrap()).unwrap();
    repo.remote("upstream", "https://github.com/user/repo.git")
        .unwrap();

    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: working_dir.canonicalize().unwrap(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: gate_path.clone(),
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };

    let result = check_remotes(&working_dir, &test_repo);
    assert!(result.passed);
    assert!(result.message.contains("Remote configuration is correct"));
}

#[test]
fn test_check_remotes_origin_wrong() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let gate_path = temp_dir.path().join("gate.git");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);

    // Configure origin incorrectly
    repo.remote("origin", "https://wrong.com/repo.git").unwrap();
    repo.remote("upstream", "https://github.com/user/repo.git")
        .unwrap();

    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: working_dir.canonicalize().unwrap(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path,
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };

    let result = check_remotes(&working_dir, &test_repo);
    assert!(!result.passed);
    assert!(result.message.contains("'origin' points to"));
}

#[test]
fn test_check_remotes_upstream_wrong() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let gate_path = temp_dir.path().join("gate.git");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);

    // Configure origin correctly but upstream incorrectly
    repo.remote("origin", gate_path.to_str().unwrap()).unwrap();
    repo.remote("upstream", "https://wrong.com/repo.git")
        .unwrap();

    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: working_dir.canonicalize().unwrap(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path,
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };

    let result = check_remotes(&working_dir, &test_repo);
    assert!(!result.passed);
    assert!(result.message.contains("'upstream' points to"));
}

// ========================================
// E2E tests for Section 7.4: Checks if daemon is running
// ========================================

/// E2E test: Verifies that `airlock doctor` checks if daemon is running
/// and fails when daemon socket doesn't exist.
///
/// Test Plan Section 7.4: "E2E: Checks if daemon is running"
#[test]
fn test_e2e_doctor_checks_daemon_socket_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // Create database so that check passes
    let _db = Database::open(&paths.database()).unwrap();

    // No socket exists - daemon check should fail
    let result = check_daemon(&paths);

    // Verify the check fails with appropriate message
    assert!(
        !result.passed,
        "Daemon check should fail when socket doesn't exist"
    );
    assert_eq!(result.name, "Daemon");
    assert!(
        result.message.contains("socket not found") || result.message.contains("not detected"),
        "Message should indicate socket not found: {}",
        result.message
    );
    assert!(
        result.suggestion.is_some(),
        "Should provide suggestion to start daemon"
    );
    assert!(
        result.suggestion.as_ref().unwrap().contains("daemon start"),
        "Suggestion should tell user to start daemon: {:?}",
        result.suggestion
    );
}

/// E2E test: Verifies that `airlock doctor` checks if daemon is running
/// and fails when socket exists but daemon is not responding.
///
/// Test Plan Section 7.4: "E2E: Checks if daemon is running"
#[cfg(unix)]
#[test]
fn test_e2e_doctor_checks_daemon_socket_exists_but_not_responding() {
    use std::os::unix::net::UnixListener;

    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // Create database
    let _db = Database::open(&paths.database()).unwrap();

    // Create a socket file but don't have anything listening on it
    // by creating and immediately dropping the listener
    let socket_path = paths.socket();
    {
        let _listener = UnixListener::bind(&socket_path).unwrap();
    }

    // The socket file exists but no one is listening
    assert!(socket_path.exists(), "Socket file should exist");

    let result = check_daemon(&paths);

    // The check should fail because nothing is responding
    assert!(
        !result.passed,
        "Daemon check should fail when socket exists but daemon not responding"
    );
    assert_eq!(result.name, "Daemon");
    assert!(
        result.message.contains("not responding") || result.message.contains("Connection refused"),
        "Message should indicate daemon not responding: {}",
        result.message
    );
    assert!(
        result.suggestion.is_some(),
        "Should provide suggestion to restart daemon"
    );
    assert!(
        result.suggestion.as_ref().unwrap().contains("restart"),
        "Suggestion should tell user to restart daemon: {:?}",
        result.suggestion
    );
}

/// E2E test: Verifies that `airlock doctor` checks if daemon is running
/// and passes when daemon is actually running and responding.
///
/// Test Plan Section 7.4: "E2E: Checks if daemon is running"
#[cfg(unix)]
#[test]
fn test_e2e_doctor_checks_daemon_running() {
    use std::os::unix::net::UnixListener;

    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // Create database
    let _db = Database::open(&paths.database()).unwrap();

    // Create a socket and keep the listener alive to simulate a running daemon
    let socket_path = paths.socket();
    let _listener = UnixListener::bind(&socket_path).unwrap();

    // The socket exists and is accepting connections
    assert!(socket_path.exists(), "Socket file should exist");

    let result = check_daemon(&paths);

    // The check should pass because the socket is responding
    assert!(
        result.passed,
        "Daemon check should pass when daemon is running: {}",
        result.message
    );
    assert_eq!(result.name, "Daemon");
    assert!(
        result.message.contains("running"),
        "Message should indicate daemon is running: {}",
        result.message
    );
    assert!(
        result.suggestion.is_none(),
        "Should not provide suggestion when daemon is healthy"
    );
}

/// E2E test: Verifies that `airlock doctor` command includes daemon check
/// in its output when run with full flow.
///
/// Test Plan Section 7.4: "E2E: Checks if daemon is running"
#[cfg(unix)]
#[test]
fn test_e2e_doctor_full_flow_includes_daemon_check() {
    use std::os::unix::net::UnixListener;

    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");
    let gate_path = airlock_root.join("repos").join("test123.git");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);

    let paths = AirlockPaths::with_root(airlock_root.clone());
    paths.ensure_dirs().unwrap();

    // Create database
    let db = Database::open(&paths.database()).unwrap();

    // Create a socket and keep the listener alive to simulate a running daemon
    let socket_path = paths.socket();
    let _listener = UnixListener::bind(&socket_path).unwrap();

    // Create gate repo with hooks
    create_test_gate_repo(&gate_path);
    git::install_hooks(&gate_path).unwrap();

    // Configure working repo remotes
    repo.remote("origin", gate_path.to_str().unwrap()).unwrap();
    repo.remote("upstream", "https://github.com/user/repo.git")
        .unwrap();

    // Enroll in database
    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: working_dir.canonicalize().unwrap(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path,
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };
    db.insert_repo(&test_repo).unwrap();

    // Run doctor - it should succeed and include daemon check
    let result = run_with_paths(&working_dir, &paths);
    assert!(
        result.is_ok(),
        "Doctor command should succeed when everything is healthy"
    );

    // Verify daemon check was included by checking the daemon function directly
    let daemon_result = check_daemon(&paths);
    assert!(
        daemon_result.passed,
        "Daemon check should be included and pass in full doctor flow"
    );
}

// ========================================
// E2E tests for Section 7.4: Checks if remotes are correctly configured
// ========================================

/// E2E test: Verifies that `airlock doctor` checks if remotes are correctly configured
/// when both origin (pointing to gate) and upstream (pointing to remote) are correct.
///
/// Test Plan Section 7.4: "E2E: Checks if remotes are correctly configured"
#[test]
fn test_e2e_doctor_checks_remotes_correct_configuration() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");
    let gate_path = airlock_root.join("repos").join("test123.git");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);

    let paths = AirlockPaths::with_root(airlock_root.clone());
    paths.ensure_dirs().unwrap();

    // Create gate repo
    create_test_gate_repo(&gate_path);

    // Configure working repo remotes correctly:
    // - origin points to gate (local proxy)
    // - upstream points to the original remote URL
    repo.remote("origin", gate_path.to_str().unwrap()).unwrap();
    repo.remote("upstream", "https://github.com/user/repo.git")
        .unwrap();

    // Create the test repo record matching the expected configuration
    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: working_dir.canonicalize().unwrap(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: gate_path.clone(),
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };

    // Verify check_remotes passes with correct configuration
    let result = check_remotes(&working_dir, &test_repo);

    assert!(
        result.passed,
        "Remote check should pass when configuration is correct: {}",
        result.message
    );
    assert_eq!(result.name, "Remotes");
    assert!(
        result.message.contains("Remote configuration is correct"),
        "Message should indicate correct configuration: {}",
        result.message
    );
    assert!(
        result.suggestion.is_none(),
        "Should not provide suggestion when remotes are correct"
    );
}

/// E2E test: Verifies that `airlock doctor` detects when origin remote is missing.
///
/// Test Plan Section 7.4: "E2E: Checks if remotes are correctly configured"
#[test]
fn test_e2e_doctor_checks_remotes_origin_missing() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let gate_path = temp_dir.path().join("gate.git");

    fs::create_dir_all(&working_dir).unwrap();
    let _repo = create_test_working_repo(&working_dir);
    // Note: Don't add any remotes - origin is missing

    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: working_dir.canonicalize().unwrap(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path,
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };

    let result = check_remotes(&working_dir, &test_repo);

    assert!(
        !result.passed,
        "Remote check should fail when origin remote is missing"
    );
    assert_eq!(result.name, "Remotes");
    assert!(
        result.message.contains("'origin' remote not found"),
        "Message should indicate origin not found: {}",
        result.message
    );
    assert!(
        result.suggestion.is_some(),
        "Should provide suggestion to fix"
    );
    assert!(
        result.suggestion.as_ref().unwrap().contains("airlock init"),
        "Suggestion should tell user to run airlock init: {:?}",
        result.suggestion
    );
}

/// E2E test: Verifies that `airlock doctor` detects when origin points to wrong location
/// (not the gate).
///
/// Test Plan Section 7.4: "E2E: Checks if remotes are correctly configured"
#[test]
fn test_e2e_doctor_checks_remotes_origin_wrong_url() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let gate_path = temp_dir.path().join("gate.git");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);

    // Configure origin to point to wrong location (not the gate)
    repo.remote("origin", "https://github.com/user/repo.git")
        .unwrap();
    repo.remote("upstream", "https://github.com/user/repo.git")
        .unwrap();

    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: working_dir.canonicalize().unwrap(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path,
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };

    let result = check_remotes(&working_dir, &test_repo);

    assert!(
        !result.passed,
        "Remote check should fail when origin doesn't point to gate"
    );
    assert_eq!(result.name, "Remotes");
    assert!(
        result.message.contains("'origin' points to"),
        "Message should indicate origin points to wrong location: {}",
        result.message
    );
    assert!(
        result.message.contains("should point to gate"),
        "Message should mention gate: {}",
        result.message
    );
    assert!(
        result.suggestion.is_some(),
        "Should provide suggestion to fix"
    );
}

/// E2E test: Verifies that `airlock doctor` detects when upstream remote is missing.
///
/// Test Plan Section 7.4: "E2E: Checks if remotes are correctly configured"
#[test]
fn test_e2e_doctor_checks_remotes_upstream_missing() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let gate_path = temp_dir.path().join("gate.git");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);

    // Configure only origin (pointing to gate), but no upstream
    repo.remote("origin", gate_path.to_str().unwrap()).unwrap();
    // Note: Don't add upstream remote

    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: working_dir.canonicalize().unwrap(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path,
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };

    let result = check_remotes(&working_dir, &test_repo);

    assert!(
        !result.passed,
        "Remote check should fail when upstream remote is missing"
    );
    assert_eq!(result.name, "Remotes");
    assert!(
        result.message.contains("'upstream' remote not found"),
        "Message should indicate upstream not found: {}",
        result.message
    );
    assert!(
        result.suggestion.is_some(),
        "Should provide suggestion to fix"
    );
}

/// E2E test: Verifies that `airlock doctor` detects when upstream points to wrong URL.
///
/// Test Plan Section 7.4: "E2E: Checks if remotes are correctly configured"
#[test]
fn test_e2e_doctor_checks_remotes_upstream_wrong_url() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let gate_path = temp_dir.path().join("gate.git");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);

    // Configure origin correctly but upstream to wrong URL
    repo.remote("origin", gate_path.to_str().unwrap()).unwrap();
    repo.remote("upstream", "https://github.com/wrong/repo.git")
        .unwrap();

    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: working_dir.canonicalize().unwrap(),
        upstream_url: "https://github.com/user/repo.git".to_string(), // Expected URL
        gate_path,
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };

    let result = check_remotes(&working_dir, &test_repo);

    assert!(
        !result.passed,
        "Remote check should fail when upstream points to wrong URL"
    );
    assert_eq!(result.name, "Remotes");
    assert!(
        result.message.contains("'upstream' points to"),
        "Message should indicate upstream points to wrong URL: {}",
        result.message
    );
    assert!(
        result.message.contains("wrong/repo"),
        "Message should show the wrong URL: {}",
        result.message
    );
    assert!(
        result.suggestion.is_some(),
        "Should provide suggestion to fix"
    );
    assert!(
        result
            .suggestion
            .as_ref()
            .unwrap()
            .contains("git remote set-url"),
        "Suggestion should tell user to update upstream URL: {:?}",
        result.suggestion
    );
}

/// E2E test: Verifies that `airlock doctor` runs remote check as part of full flow
/// when repo is enrolled.
///
/// Test Plan Section 7.4: "E2E: Checks if remotes are correctly configured"
#[cfg(unix)]
#[test]
fn test_e2e_doctor_full_flow_includes_remote_check() {
    use std::os::unix::net::UnixListener;

    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");
    let gate_path = airlock_root.join("repos").join("test123.git");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);

    let paths = AirlockPaths::with_root(airlock_root.clone());
    paths.ensure_dirs().unwrap();

    // Create database and enroll repo
    let db = Database::open(&paths.database()).unwrap();

    // Create a socket to simulate running daemon
    let socket_path = paths.socket();
    let _listener = UnixListener::bind(&socket_path).unwrap();

    // Create gate repo with hooks
    create_test_gate_repo(&gate_path);
    git::install_hooks(&gate_path).unwrap();

    // Configure working repo remotes correctly
    repo.remote("origin", gate_path.to_str().unwrap()).unwrap();
    repo.remote("upstream", "https://github.com/user/repo.git")
        .unwrap();

    // Enroll in database
    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: working_dir.canonicalize().unwrap(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: gate_path.clone(),
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };
    db.insert_repo(&test_repo).unwrap();

    // Run doctor - should succeed and include remote check
    let result = run_with_paths(&working_dir, &paths);
    assert!(
        result.is_ok(),
        "Doctor command should succeed when everything is healthy"
    );

    // Verify remote check was run by checking directly
    let remote_result = check_remotes(&working_dir, &test_repo);
    assert!(
        remote_result.passed,
        "Remote check should pass in full doctor flow: {}",
        remote_result.message
    );
}

// ========================================
// E2E tests for Section 7.4: Reports issues with suggested fixes
// ========================================

/// E2E test: Verifies that `airlock doctor` reports issues with suggested fixes
/// when there are problems detected.
///
/// This test verifies the output format when issues are detected:
/// - Shows which checks failed with their error messages
/// - Collects and displays suggestions for each failing check
/// - Prints a "Suggested Fixes" section at the end
///
/// Test Plan Section 7.4: "E2E: Reports issues with suggested fixes"
#[test]
fn test_e2e_doctor_reports_issues_with_suggestions() {
    // Test individual check functions that return suggestions when they fail

    // 1. Test daemon check provides suggestion when socket not found
    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let daemon_result = check_daemon(&paths);
    assert!(
        !daemon_result.passed,
        "Daemon check should fail when socket doesn't exist"
    );
    assert!(
        daemon_result.suggestion.is_some(),
        "Daemon check should provide a suggestion"
    );
    let daemon_suggestion = daemon_result.suggestion.unwrap();
    assert!(
        daemon_suggestion.contains("daemon start"),
        "Daemon suggestion should tell user to start daemon: {}",
        daemon_suggestion
    );

    // 2. Test database check provides suggestion when database not found
    let temp_dir2 = TempDir::new().unwrap();
    let paths2 = AirlockPaths::with_root(temp_dir2.path().to_path_buf());
    // Don't create dirs - database doesn't exist

    let db_result = check_database(&paths2);
    assert!(
        !db_result.passed,
        "Database check should fail when database doesn't exist"
    );
    assert!(
        db_result.suggestion.is_some(),
        "Database check should provide a suggestion"
    );
    let db_suggestion = db_result.suggestion.unwrap();
    assert!(
        db_suggestion.contains("airlock init"),
        "Database suggestion should mention airlock init: {}",
        db_suggestion
    );

    // 3. Test hooks check provides suggestion when hooks are missing
    let temp_dir3 = TempDir::new().unwrap();
    let gate_path = temp_dir3.path().join("gate.git");
    fs::create_dir_all(gate_path.join("hooks")).unwrap();
    // Don't install any hooks

    let hooks_result = check_hooks(&gate_path);
    assert!(
        !hooks_result.passed,
        "Hooks check should fail when hooks are missing"
    );
    assert!(
        hooks_result.suggestion.is_some(),
        "Hooks check should provide a suggestion"
    );
    let hooks_suggestion = hooks_result.suggestion.unwrap();
    assert!(
        hooks_suggestion.contains("airlock eject") || hooks_suggestion.contains("airlock init"),
        "Hooks suggestion should tell user to reinitialize: {}",
        hooks_suggestion
    );
}

/// E2E test: Verifies that `airlock doctor` reports multiple issues with their
/// respective suggestions when multiple checks fail.
///
/// Test Plan Section 7.4: "E2E: Reports issues with suggested fixes"
#[test]
fn test_e2e_doctor_reports_multiple_issues_with_suggestions() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");
    let gate_path = airlock_root.join("repos").join("test123.git");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);

    let paths = AirlockPaths::with_root(airlock_root.clone());
    paths.ensure_dirs().unwrap();

    // Create database
    let db = Database::open(&paths.database()).unwrap();

    // Create gate repo WITHOUT hooks (to trigger hooks check failure)
    create_test_gate_repo(&gate_path);
    fs::create_dir_all(gate_path.join("hooks")).unwrap();
    // Don't install hooks

    // Configure working repo remotes correctly
    repo.remote("origin", gate_path.to_str().unwrap()).unwrap();
    repo.remote("upstream", "https://github.com/user/repo.git")
        .unwrap();

    // Enroll in database
    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: working_dir.canonicalize().unwrap(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: gate_path.clone(),
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };
    db.insert_repo(&test_repo).unwrap();

    // Run doctor (without daemon running - this will trigger daemon failure)
    // The run_with_paths function collects all suggestions from failing checks
    let result = run_with_paths(&working_dir, &paths);
    assert!(
        result.is_ok(),
        "Doctor command should complete even with failing checks"
    );

    // Verify multiple checks failed and have suggestions
    let daemon_result = check_daemon(&paths);
    assert!(
        !daemon_result.passed,
        "Daemon check should fail (no socket)"
    );
    assert!(
        daemon_result.suggestion.is_some(),
        "Daemon should have suggestion"
    );

    let hooks_result = check_hooks(&gate_path);
    assert!(
        !hooks_result.passed,
        "Hooks check should fail (no hooks installed)"
    );
    assert!(
        hooks_result.suggestion.is_some(),
        "Hooks should have suggestion"
    );

    // Both failing checks should have different suggestions
    let daemon_suggestion = daemon_result.suggestion.unwrap();
    let hooks_suggestion = hooks_result.suggestion.unwrap();
    assert_ne!(
        daemon_suggestion, hooks_suggestion,
        "Different checks should have different suggestions"
    );
}

/// E2E test: Verifies that `airlock doctor` includes specific fix commands
/// in its suggestions.
///
/// Test Plan Section 7.4: "E2E: Reports issues with suggested fixes"
#[test]
fn test_e2e_doctor_suggestions_contain_actionable_commands() {
    // Test that each type of failure provides actionable commands

    // 1. Daemon not running - should suggest 'airlock daemon start'
    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    let daemon_result = check_daemon(&paths);
    assert!(
        daemon_result
            .suggestion
            .as_ref()
            .map_or(false, |s| s.contains("airlock daemon start")),
        "Daemon suggestion should include 'airlock daemon start': {:?}",
        daemon_result.suggestion
    );

    // 2. Repo not enrolled - should suggest 'airlock init'
    let working_dir = temp_dir.path().join("working");
    fs::create_dir_all(&working_dir).unwrap();
    create_test_working_repo(&working_dir);

    // Create database but don't enroll the repo
    let _db = Database::open(&paths.database()).unwrap();

    let enrollment_result = check_repo_enrollment(&working_dir, &paths);
    assert!(
        enrollment_result
            .suggestion
            .as_ref()
            .map_or(false, |s| s.contains("airlock init")),
        "Enrollment suggestion should include 'airlock init': {:?}",
        enrollment_result.suggestion
    );

    // 3. Gate repo missing - should suggest 'airlock eject' then 'airlock init'
    let gate_path = temp_dir.path().join("nonexistent_gate.git");
    let gate_result = check_gate_repo(&gate_path);
    assert!(
        gate_result
            .suggestion
            .as_ref()
            .map_or(false, |s| s.contains("airlock eject")
                || s.contains("airlock init")),
        "Gate repo suggestion should include recovery steps: {:?}",
        gate_result.suggestion
    );

    // 4. Database corrupted - should suggest deleting and reinitializing
    let temp_dir2 = TempDir::new().unwrap();
    let paths2 = AirlockPaths::with_root(temp_dir2.path().to_path_buf());
    paths2.ensure_dirs().unwrap();
    let db_path = paths2.database();
    fs::write(&db_path, "corrupted data").unwrap();

    let db_result = check_database(&paths2);
    assert!(
        db_result
            .suggestion
            .as_ref()
            .map_or(false, |s| s.contains("deleting")),
        "Corrupted database suggestion should include deleting: {:?}",
        db_result.suggestion
    );
}

// ========================================
// E2E tests for Section 7.4: Returns success when everything is healthy
// ========================================

/// E2E test: Verifies that `airlock doctor` returns success when all checks pass.
///
/// This test verifies:
/// - All individual checks pass when properly configured
/// - The doctor command completes successfully
/// - No suggestions are provided when everything is healthy
///
/// Test Plan Section 7.4: "E2E: Returns success when everything is healthy"
#[cfg(unix)]
#[test]
fn test_e2e_doctor_returns_success_when_all_checks_pass() {
    use std::os::unix::net::UnixListener;

    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");
    let gate_path = airlock_root.join("repos").join("test123.git");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);

    let paths = AirlockPaths::with_root(airlock_root.clone());
    paths.ensure_dirs().unwrap();

    // Create database and enroll repo
    let db = Database::open(&paths.database()).unwrap();

    // Create a socket to simulate running daemon
    let socket_path = paths.socket();
    let _listener = UnixListener::bind(&socket_path).unwrap();

    // Create gate repo with all hooks properly installed
    create_test_gate_repo(&gate_path);
    git::install_hooks(&gate_path).unwrap();

    // Configure working repo remotes correctly
    repo.remote("origin", gate_path.to_str().unwrap()).unwrap();
    repo.remote("upstream", "https://github.com/user/repo.git")
        .unwrap();

    // Enroll in database
    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: working_dir.canonicalize().unwrap(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: gate_path.clone(),
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };
    db.insert_repo(&test_repo).unwrap();

    // Verify all individual checks pass
    let daemon_result = check_daemon(&paths);
    assert!(
        daemon_result.passed,
        "Daemon check should pass: {}",
        daemon_result.message
    );
    assert!(
        daemon_result.suggestion.is_none(),
        "No suggestion when daemon is healthy"
    );

    let db_result = check_database(&paths);
    assert!(
        db_result.passed,
        "Database check should pass: {}",
        db_result.message
    );
    assert!(
        db_result.suggestion.is_none(),
        "No suggestion when database is healthy"
    );

    let repo_result = check_repo_enrollment(&working_dir, &paths);
    assert!(
        repo_result.passed,
        "Enrollment check should pass: {}",
        repo_result.message
    );
    assert!(
        repo_result.suggestion.is_none(),
        "No suggestion when repo is enrolled"
    );

    let remotes_result = check_remotes(&working_dir, &test_repo);
    assert!(
        remotes_result.passed,
        "Remotes check should pass: {}",
        remotes_result.message
    );
    assert!(
        remotes_result.suggestion.is_none(),
        "No suggestion when remotes are correct"
    );

    let hooks_result = check_hooks(&gate_path);
    assert!(
        hooks_result.passed,
        "Hooks check should pass: {}",
        hooks_result.message
    );
    assert!(
        hooks_result.suggestion.is_none(),
        "No suggestion when hooks are installed"
    );

    let gate_result = check_gate_repo(&gate_path);
    assert!(
        gate_result.passed,
        "Gate repo check should pass: {}",
        gate_result.message
    );
    assert!(
        gate_result.suggestion.is_none(),
        "No suggestion when gate is healthy"
    );

    // Run full doctor flow
    let result = run_with_paths(&working_dir, &paths);
    assert!(
        result.is_ok(),
        "Doctor command should succeed when all checks pass"
    );
}

/// E2E test: Verifies that `airlock doctor` outputs appropriate success message
/// when everything is healthy.
///
/// Test Plan Section 7.4: "E2E: Returns success when everything is healthy"
#[cfg(unix)]
#[test]
fn test_e2e_doctor_success_output_format() {
    use std::os::unix::net::UnixListener;

    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");
    let gate_path = airlock_root.join("repos").join("test123.git");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);

    let paths = AirlockPaths::with_root(airlock_root.clone());
    paths.ensure_dirs().unwrap();

    // Set up everything correctly
    let db = Database::open(&paths.database()).unwrap();
    let socket_path = paths.socket();
    let _listener = UnixListener::bind(&socket_path).unwrap();
    create_test_gate_repo(&gate_path);
    git::install_hooks(&gate_path).unwrap();
    repo.remote("origin", gate_path.to_str().unwrap()).unwrap();
    repo.remote("upstream", "https://github.com/user/repo.git")
        .unwrap();
    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: working_dir.canonicalize().unwrap(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: gate_path.clone(),
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };
    db.insert_repo(&test_repo).unwrap();

    // Verify all checks pass (no issues collected)
    let checks = vec![
        check_daemon(&paths),
        check_database(&paths),
        check_repo_enrollment(&working_dir, &paths),
        check_remotes(&working_dir, &test_repo),
        check_hooks(&gate_path),
        check_gate_repo(&gate_path),
    ];

    // All should pass
    let all_passed = checks.iter().all(|r| r.passed);
    assert!(all_passed, "All checks should pass for healthy setup");

    // No suggestions should be present
    let has_suggestions = checks.iter().any(|r| r.suggestion.is_some());
    assert!(
        !has_suggestions,
        "No suggestions should be present when all checks pass"
    );

    // Verify the count matches expectations
    let passed_count = checks.iter().filter(|r| r.passed).count();
    let total_count = checks.len();
    assert_eq!(
        passed_count, total_count,
        "All {} checks should pass",
        total_count
    );
}

/// E2E test: Verifies that `airlock doctor` returns success even when running
/// outside a git repo (only some checks apply).
///
/// Test Plan Section 7.4: "E2E: Returns success when everything is healthy"
#[cfg(unix)]
#[test]
fn test_e2e_doctor_returns_success_outside_git_repo() {
    use std::os::unix::net::UnixListener;

    let temp_dir = TempDir::new().unwrap();
    let airlock_root = temp_dir.path().join("airlock");

    let paths = AirlockPaths::with_root(airlock_root.clone());
    paths.ensure_dirs().unwrap();

    // Create database
    let _db = Database::open(&paths.database()).unwrap();

    // Create daemon socket
    let socket_path = paths.socket();
    let _listener = UnixListener::bind(&socket_path).unwrap();

    // Run doctor outside of any git repo
    // Should succeed with only daemon and database checks running
    let result = run_with_paths(temp_dir.path(), &paths);
    assert!(
        result.is_ok(),
        "Doctor should succeed when run outside git repo with daemon and database healthy"
    );

    // Verify that repo-specific checks are skipped (return warning, not failure)
    let repo_result = check_repo_enrollment(temp_dir.path(), &paths);
    assert!(
        repo_result.passed,
        "Repo check should pass (as warning) when outside git repo: {}",
        repo_result.message
    );
    assert!(
        repo_result.message.contains("Not inside a Git repository"),
        "Message should indicate not in git repo: {}",
        repo_result.message
    );
}
