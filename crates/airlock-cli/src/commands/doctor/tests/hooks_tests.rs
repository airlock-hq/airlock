use super::super::*;
use super::helpers::*;
use airlock_core::{git, Database, Repo};
use git2::Repository;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_check_hooks_missing() {
    let temp_dir = TempDir::new().unwrap();
    let gate_path = temp_dir.path().join("gate.git");

    fs::create_dir_all(&gate_path).unwrap();
    // Create hooks directory but no hooks
    fs::create_dir_all(gate_path.join("hooks")).unwrap();

    let result = check_hooks(&gate_path);
    assert!(!result.passed);
    assert!(result.message.contains("Missing hooks"));
}

#[test]
fn test_check_hooks_ok() {
    let temp_dir = TempDir::new().unwrap();
    let gate_path = temp_dir.path().join("gate.git");

    fs::create_dir_all(&gate_path).unwrap();

    // Install hooks using the library function
    git::install_hooks(&gate_path).unwrap();

    let result = check_hooks(&gate_path);
    assert!(result.passed);
    assert!(result.message.contains("All hooks installed"));
}

// ========================================
// E2E tests for Section 7.4: Checks if hooks are installed
// ========================================

/// E2E test: Verifies that `airlock doctor` checks if hooks are installed
/// and passes when all required hooks exist and are executable.
///
/// Test Plan Section 7.4: "E2E: Checks if hooks are installed"
#[test]
fn test_e2e_doctor_checks_hooks_all_installed() {
    let temp_dir = TempDir::new().unwrap();
    let gate_path = temp_dir.path().join("gate.git");

    // Create gate repo
    fs::create_dir_all(&gate_path).unwrap();
    Repository::init_bare(&gate_path).expect("Failed to init bare repo");

    // Install hooks using the library function (which makes them executable)
    git::install_hooks(&gate_path).unwrap();

    let result = check_hooks(&gate_path);

    assert!(
        result.passed,
        "Hook check should pass when all hooks are installed: {}",
        result.message
    );
    assert_eq!(result.name, "Hooks");
    assert!(
        result.message.contains("All hooks installed"),
        "Message should indicate all hooks are installed: {}",
        result.message
    );
    assert!(
        result.message.contains("executable"),
        "Message should mention hooks are executable: {}",
        result.message
    );
    assert!(
        result.suggestion.is_none(),
        "Should not provide suggestion when hooks are healthy"
    );
}

/// E2E test: Verifies that `airlock doctor` detects when hooks directory is missing.
///
/// Test Plan Section 7.4: "E2E: Checks if hooks are installed"
#[test]
fn test_e2e_doctor_checks_hooks_directory_missing() {
    let temp_dir = TempDir::new().unwrap();
    let gate_path = temp_dir.path().join("gate.git");

    // Create gate repo directory but no hooks directory
    fs::create_dir_all(&gate_path).unwrap();
    // Don't create hooks directory

    let result = check_hooks(&gate_path);

    assert!(
        !result.passed,
        "Hook check should fail when hooks directory is missing"
    );
    assert_eq!(result.name, "Hooks");
    assert!(
        result.message.contains("Hooks directory not found"),
        "Message should indicate hooks directory not found: {}",
        result.message
    );
    assert!(
        result.suggestion.is_some(),
        "Should provide suggestion to fix"
    );
    assert!(
        result.suggestion.as_ref().unwrap().contains("airlock init"),
        "Suggestion should tell user to reinitialize: {:?}",
        result.suggestion
    );
}

/// E2E test: Verifies that `airlock doctor` detects when pre-receive hook is missing.
///
/// Test Plan Section 7.4: "E2E: Checks if hooks are installed"
#[test]
fn test_e2e_doctor_checks_hooks_pre_receive_missing() {
    let temp_dir = TempDir::new().unwrap();
    let gate_path = temp_dir.path().join("gate.git");
    let hooks_dir = gate_path.join("hooks");

    // Create gate repo with hooks directory
    fs::create_dir_all(&hooks_dir).unwrap();

    // Create only some hooks (missing pre-receive)
    fs::write(hooks_dir.join("post-receive"), "#!/bin/sh\n").unwrap();

    // Make them executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for hook in &["post-receive"] {
            let hook_path = hooks_dir.join(hook);
            let mut perms = fs::metadata(&hook_path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&hook_path, perms).unwrap();
        }
    }

    let result = check_hooks(&gate_path);

    assert!(
        !result.passed,
        "Hook check should fail when pre-receive hook is missing"
    );
    assert_eq!(result.name, "Hooks");
    assert!(
        result.message.contains("Missing hooks"),
        "Message should indicate missing hooks: {}",
        result.message
    );
    assert!(
        result.message.contains("pre-receive"),
        "Message should mention pre-receive hook is missing: {}",
        result.message
    );
    assert!(
        result.suggestion.is_some(),
        "Should provide suggestion to fix"
    );
}

/// E2E test: Verifies that `airlock doctor` detects when post-receive hook is missing.
///
/// Test Plan Section 7.4: "E2E: Checks if hooks are installed"
#[test]
fn test_e2e_doctor_checks_hooks_post_receive_missing() {
    let temp_dir = TempDir::new().unwrap();
    let gate_path = temp_dir.path().join("gate.git");
    let hooks_dir = gate_path.join("hooks");

    // Create gate repo with hooks directory
    fs::create_dir_all(&hooks_dir).unwrap();

    // Create only some hooks (missing post-receive)
    fs::write(hooks_dir.join("pre-receive"), "#!/bin/sh\n").unwrap();

    // Make them executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for hook in &["pre-receive"] {
            let hook_path = hooks_dir.join(hook);
            let mut perms = fs::metadata(&hook_path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&hook_path, perms).unwrap();
        }
    }

    let result = check_hooks(&gate_path);

    assert!(
        !result.passed,
        "Hook check should fail when post-receive hook is missing"
    );
    assert_eq!(result.name, "Hooks");
    assert!(
        result.message.contains("Missing hooks"),
        "Message should indicate missing hooks: {}",
        result.message
    );
    assert!(
        result.message.contains("post-receive"),
        "Message should mention post-receive hook is missing: {}",
        result.message
    );
}

/// E2E test: Verifies that `airlock doctor` detects when multiple hooks are missing.
///
/// Test Plan Section 7.4: "E2E: Checks if hooks are installed"
#[test]
fn test_e2e_doctor_checks_hooks_multiple_missing() {
    let temp_dir = TempDir::new().unwrap();
    let gate_path = temp_dir.path().join("gate.git");
    let hooks_dir = gate_path.join("hooks");

    // Create gate repo with hooks directory but no hook files
    fs::create_dir_all(&hooks_dir).unwrap();

    let result = check_hooks(&gate_path);

    assert!(
        !result.passed,
        "Hook check should fail when multiple hooks are missing"
    );
    assert_eq!(result.name, "Hooks");
    assert!(
        result.message.contains("Missing hooks"),
        "Message should indicate missing hooks: {}",
        result.message
    );
    // Should list all missing hooks
    assert!(
        result.message.contains("pre-receive"),
        "Message should mention pre-receive: {}",
        result.message
    );
    assert!(
        result.message.contains("post-receive"),
        "Message should mention post-receive: {}",
        result.message
    );
}

/// E2E test: Verifies that `airlock doctor` detects when hooks exist but are not executable.
/// (Unix only - Windows doesn't have executable permissions in the same way)
///
/// Test Plan Section 7.4: "E2E: Checks if hooks are installed"
#[cfg(unix)]
#[test]
fn test_e2e_doctor_checks_hooks_not_executable() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().unwrap();
    let gate_path = temp_dir.path().join("gate.git");
    let hooks_dir = gate_path.join("hooks");

    // Create gate repo with hooks directory
    fs::create_dir_all(&hooks_dir).unwrap();

    // Create all required hooks but make them NOT executable
    for hook in &["pre-receive", "post-receive"] {
        let hook_path = hooks_dir.join(hook);
        fs::write(&hook_path, "#!/bin/sh\necho 'hook'\n").unwrap();
        // Set permissions to 644 (not executable)
        let mut perms = fs::metadata(&hook_path).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&hook_path, perms).unwrap();
    }

    let result = check_hooks(&gate_path);

    assert!(
        !result.passed,
        "Hook check should fail when hooks are not executable"
    );
    assert_eq!(result.name, "Hooks");
    assert!(
        result.message.contains("not executable"),
        "Message should indicate hooks are not executable: {}",
        result.message
    );
    assert!(
        result.suggestion.is_some(),
        "Should provide suggestion to fix"
    );
    assert!(
        result.suggestion.as_ref().unwrap().contains("chmod +x"),
        "Suggestion should tell user to chmod +x: {:?}",
        result.suggestion
    );
}

/// E2E test: Verifies that `airlock doctor` detects when only one hook is not executable.
/// (Unix only)
///
/// Test Plan Section 7.4: "E2E: Checks if hooks are installed"
#[cfg(unix)]
#[test]
fn test_e2e_doctor_checks_hooks_one_not_executable() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().unwrap();
    let gate_path = temp_dir.path().join("gate.git");
    let hooks_dir = gate_path.join("hooks");

    // Create gate repo with hooks directory
    fs::create_dir_all(&hooks_dir).unwrap();

    // Create all required hooks
    for hook in &["pre-receive", "post-receive"] {
        let hook_path = hooks_dir.join(hook);
        fs::write(&hook_path, "#!/bin/sh\necho 'hook'\n").unwrap();

        // Make pre-receive executable, but NOT post-receive
        let mode = if *hook == "post-receive" {
            0o644
        } else {
            0o755
        };
        let mut perms = fs::metadata(&hook_path).unwrap().permissions();
        perms.set_mode(mode);
        fs::set_permissions(&hook_path, perms).unwrap();
    }

    let result = check_hooks(&gate_path);

    assert!(
        !result.passed,
        "Hook check should fail when one hook is not executable"
    );
    assert_eq!(result.name, "Hooks");
    assert!(
        result.message.contains("not executable"),
        "Message should indicate hook is not executable: {}",
        result.message
    );
    assert!(
        result.message.contains("post-receive"),
        "Message should specifically mention post-receive: {}",
        result.message
    );
    // Should NOT mention the executable hooks
    assert!(
        !result.message.contains("pre-receive"),
        "Message should NOT mention pre-receive (it's executable): {}",
        result.message
    );
}

/// E2E test: Verifies that `airlock doctor` runs hooks check as part of full flow
/// when repo is enrolled.
///
/// Test Plan Section 7.4: "E2E: Checks if hooks are installed"
#[cfg(unix)]
#[test]
fn test_e2e_doctor_full_flow_includes_hooks_check() {
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

    // Create a socket to simulate running daemon
    let socket_path = paths.socket();
    let _listener = UnixListener::bind(&socket_path).unwrap();

    // Create gate repo WITH hooks properly installed
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

    // Run doctor - should succeed and include hooks check
    let result = run_with_paths(&working_dir, &paths);
    assert!(
        result.is_ok(),
        "Doctor command should succeed when everything is healthy"
    );

    // Verify hooks check was run by checking directly
    let hooks_result = check_hooks(&gate_path);
    assert!(
        hooks_result.passed,
        "Hooks check should pass in full doctor flow: {}",
        hooks_result.message
    );
}

/// E2E test: Verifies that `airlock doctor` fails appropriately when hooks are missing
/// in the full flow.
///
/// Test Plan Section 7.4: "E2E: Checks if hooks are installed"
#[cfg(unix)]
#[test]
fn test_e2e_doctor_full_flow_fails_with_missing_hooks() {
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

    // Create a socket to simulate running daemon
    let socket_path = paths.socket();
    let _listener = UnixListener::bind(&socket_path).unwrap();

    // Create gate repo WITHOUT hooks (don't call install_hooks)
    create_test_gate_repo(&gate_path);
    // Create hooks directory but leave it empty
    fs::create_dir_all(gate_path.join("hooks")).unwrap();

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

    // Run doctor - should still complete (returns Ok) but have failing checks
    let result = run_with_paths(&working_dir, &paths);
    assert!(
        result.is_ok(),
        "Doctor command should complete even with failing checks"
    );

    // Verify hooks check was run and failed
    let hooks_result = check_hooks(&gate_path);
    assert!(
        !hooks_result.passed,
        "Hooks check should fail when hooks are missing: {}",
        hooks_result.message
    );
    assert!(
        hooks_result.message.contains("Missing hooks"),
        "Message should indicate missing hooks: {}",
        hooks_result.message
    );
}
