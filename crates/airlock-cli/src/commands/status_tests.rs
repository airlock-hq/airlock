use super::*;
use crate::commands::doctor::tests::helpers::{create_test_working_repo, now_timestamp};
use airlock_core::{JobResult, JobStatus, Repo, Run, StepResult, StepStatus};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_format_time_ago() {
    let now = now_timestamp();

    assert_eq!(format_time_ago(now - 30), "30s ago");
    assert_eq!(format_time_ago(now - 120), "2m ago");
    assert_eq!(format_time_ago(now - 3600), "1h ago");
    assert_eq!(format_time_ago(now - 86400), "1d ago");
    assert_eq!(format_time_ago(now - 86400 * 45), "1mo ago");
}

#[test]
fn test_status_fails_outside_git_repo() {
    let temp_dir = TempDir::new().unwrap();
    let airlock_root = temp_dir.path().join("airlock");
    let paths = AirlockPaths::with_root(airlock_root);

    let result = run_with_paths(temp_dir.path(), &paths);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Not inside a Git repository"));
}

#[test]
fn test_status_fails_if_not_enrolled() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();
    create_test_working_repo(&working_dir);

    let paths = AirlockPaths::with_root(airlock_root);
    paths.ensure_dirs().unwrap();

    // Initialize database but don't enroll the repo
    let _db = Database::open(&paths.database()).unwrap();

    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("not enrolled in Airlock"));
}

#[test]
fn test_status_shows_enrolled_repo() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);

    // Add an origin remote to the repo
    repo.remote("origin", "https://github.com/user/repo.git")
        .unwrap();

    let paths = AirlockPaths::with_root(airlock_root);
    paths.ensure_dirs().unwrap();

    // Enroll the repo in the database
    let db = Database::open(&paths.database()).unwrap();
    let canonical_path = working_dir.canonicalize().unwrap();
    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: canonical_path.clone(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: PathBuf::from("/tmp/gate.git"),
        last_sync: Some(now_timestamp() - 3600), // 1 hour ago
        created_at: now_timestamp() - 86400,     // 1 day ago
    };
    db.insert_repo(&test_repo).unwrap();

    // Run status - should succeed
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "Status failed: {:?}", result.err());
}

#[test]
fn test_status_shows_active_runs() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);
    repo.remote("origin", "https://github.com/user/repo.git")
        .unwrap();

    let paths = AirlockPaths::with_root(airlock_root);
    paths.ensure_dirs().unwrap();

    let db = Database::open(&paths.database()).unwrap();
    let canonical_path = working_dir.canonicalize().unwrap();

    // Create repo
    let test_repo = Repo {
        id: "test123".to_string(),
        working_path: canonical_path.clone(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: PathBuf::from("/tmp/gate.git"),
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };
    db.insert_repo(&test_repo).unwrap();

    // Create some runs with stage results for derived status
    let created = now_timestamp();
    let run1 = Run {
        id: "run1".to_string(),
        repo_id: "test123".to_string(),
        ref_updates: vec![],
        error: None,
        superseded: false,
        created_at: created,
        branch: String::new(),
        base_sha: String::new(),
        head_sha: String::new(),
        current_step: None,
        workflow_file: String::new(),
        workflow_name: None,
        updated_at: created,
    };
    db.insert_run(&run1).unwrap();
    // Create job result first (FK constraint)
    db.insert_job_result(&JobResult {
        id: "job1".to_string(),
        run_id: "run1".to_string(),
        job_key: "default".to_string(),
        name: Some("default".to_string()),
        status: JobStatus::Running,
        job_order: 0,
        started_at: None,
        completed_at: None,
        error: None,
    })
    .unwrap();
    // Give run1 a running stage
    db.insert_step_result(&StepResult {
        id: "sr1".to_string(),
        run_id: "run1".to_string(),
        job_id: "job1".to_string(),
        name: "test".to_string(),
        status: StepStatus::Running,
        step_order: 0,
        exit_code: None,
        duration_ms: None,
        error: None,
        started_at: None,
        completed_at: None,
    })
    .unwrap();

    let run2 = Run {
        id: "run2".to_string(),
        repo_id: "test123".to_string(),
        ref_updates: vec![],
        error: None,
        superseded: false,
        created_at: created,
        branch: String::new(),
        base_sha: String::new(),
        head_sha: String::new(),
        current_step: None,
        workflow_file: String::new(),
        workflow_name: None,
        updated_at: created,
    };
    db.insert_run(&run2).unwrap();
    // Create job result first (FK constraint)
    db.insert_job_result(&JobResult {
        id: "job2".to_string(),
        run_id: "run2".to_string(),
        job_key: "default".to_string(),
        name: Some("default".to_string()),
        status: JobStatus::Pending,
        job_order: 0,
        started_at: None,
        completed_at: None,
        error: None,
    })
    .unwrap();
    // Give run2 a pending stage (no stages = active)
    db.insert_step_result(&StepResult {
        id: "sr2".to_string(),
        run_id: "run2".to_string(),
        job_id: "job2".to_string(),
        name: "test".to_string(),
        status: StepStatus::Pending,
        step_order: 0,
        exit_code: None,
        duration_ms: None,
        error: None,
        started_at: None,
        completed_at: None,
    })
    .unwrap();

    // Run status
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "Status failed: {:?}", result.err());
}

/// E2E test verifying that `airlock status` shows pending runs count correctly.
///
/// Status is now derived from stage results. This test creates runs with
/// different stage states and verifies they're counted correctly.
#[test]
fn test_e2e_status_shows_pending_runs_count() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);
    repo.remote("origin", "https://github.com/user/repo.git")
        .unwrap();

    let paths = AirlockPaths::with_root(airlock_root);
    paths.ensure_dirs().unwrap();

    let db = Database::open(&paths.database()).unwrap();
    let canonical_path = working_dir.canonicalize().unwrap();

    // Create repo
    let test_repo = Repo {
        id: "test_pending".to_string(),
        working_path: canonical_path.clone(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: PathBuf::from("/tmp/gate.git"),
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };
    db.insert_repo(&test_repo).unwrap();

    // Create 3 awaiting-approval runs (via stage results)
    for i in 1..=3 {
        let created = now_timestamp() - i as i64;
        let run = Run {
            id: format!("awaiting_run_{}", i),
            repo_id: "test_pending".to_string(),
            ref_updates: vec![],
            error: None,
            superseded: false,
            created_at: created,
            branch: String::new(),
            base_sha: String::new(),
            head_sha: String::new(),
            current_step: None,
            workflow_file: String::new(),
            workflow_name: None,
            updated_at: created,
        };
        db.insert_run(&run).unwrap();
        db.insert_job_result(&JobResult {
            id: format!("job_await_{}", i),
            run_id: format!("awaiting_run_{}", i),
            job_key: "default".to_string(),
            name: Some("default".to_string()),
            status: JobStatus::Running,
            job_order: 0,
            started_at: None,
            completed_at: None,
            error: None,
        })
        .unwrap();
        db.insert_step_result(&StepResult {
            id: format!("sr_await_{}", i),
            run_id: format!("awaiting_run_{}", i),
            job_id: format!("job_await_{}", i),
            name: "review".to_string(),
            status: StepStatus::AwaitingApproval,
            step_order: 0,
            exit_code: None,
            duration_ms: None,
            error: None,
            started_at: None,
            completed_at: None,
        })
        .unwrap();
    }

    // Create 2 running runs (via stage results)
    for i in 1..=2 {
        let created = now_timestamp() - 10 - i as i64;
        let run = Run {
            id: format!("running_run_{}", i),
            repo_id: "test_pending".to_string(),
            ref_updates: vec![],
            error: None,
            superseded: false,
            created_at: created,
            branch: String::new(),
            base_sha: String::new(),
            head_sha: String::new(),
            current_step: None,
            workflow_file: String::new(),
            workflow_name: None,
            updated_at: created,
        };
        db.insert_run(&run).unwrap();
        db.insert_job_result(&JobResult {
            id: format!("job_run_{}", i),
            run_id: format!("running_run_{}", i),
            job_key: "default".to_string(),
            name: Some("default".to_string()),
            status: JobStatus::Running,
            job_order: 0,
            started_at: None,
            completed_at: None,
            error: None,
        })
        .unwrap();
        db.insert_step_result(&StepResult {
            id: format!("sr_run_{}", i),
            run_id: format!("running_run_{}", i),
            job_id: format!("job_run_{}", i),
            name: "test".to_string(),
            status: StepStatus::Running,
            step_order: 0,
            exit_code: None,
            duration_ms: None,
            error: None,
            started_at: None,
            completed_at: None,
        })
        .unwrap();
    }

    // Create 1 completed run (should NOT be in active runs)
    let created = now_timestamp() - 100;
    let completed_run = Run {
        id: "completed_run_1".to_string(),
        repo_id: "test_pending".to_string(),
        ref_updates: vec![],
        error: None,
        superseded: false,
        created_at: created,
        branch: String::new(),
        base_sha: String::new(),
        head_sha: String::new(),
        current_step: None,
        workflow_file: String::new(),
        workflow_name: None,
        updated_at: created,
    };
    db.insert_run(&completed_run).unwrap();
    db.insert_job_result(&JobResult {
        id: "job_complete".to_string(),
        run_id: "completed_run_1".to_string(),
        job_key: "default".to_string(),
        name: Some("default".to_string()),
        status: JobStatus::Passed,
        job_order: 0,
        started_at: None,
        completed_at: None,
        error: None,
    })
    .unwrap();
    db.insert_step_result(&StepResult {
        id: "sr_complete".to_string(),
        run_id: "completed_run_1".to_string(),
        job_id: "job_complete".to_string(),
        name: "test".to_string(),
        status: StepStatus::Passed,
        step_order: 0,
        exit_code: Some(0),
        duration_ms: None,
        error: None,
        started_at: None,
        completed_at: None,
    })
    .unwrap();

    // Verify the database state
    let active_runs = db.list_active_runs(&test_repo.id).unwrap();
    assert_eq!(
        active_runs.len(),
        5,
        "Expected 5 active runs (3 awaiting + 2 running)"
    );

    // Run the status command - should succeed
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "Status command failed: {:?}", result.err());
}

/// E2E test verifying that `airlock status` shows last sync timestamp correctly.
///
/// This test validates Section 7.3 of the test plan:
/// - "E2E: Shows last sync timestamp"
///
/// The test creates a repo with a known last_sync timestamp and verifies:
/// 1. The repo is correctly stored with the last_sync timestamp
/// 2. The status command succeeds when last_sync is present
/// 3. The format_time_ago function correctly formats relative time
/// 4. The command also handles repos that have never synced (last_sync = None)
#[test]
fn test_e2e_status_shows_last_sync_timestamp() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);
    repo.remote("origin", "https://github.com/user/repo.git")
        .unwrap();

    let paths = AirlockPaths::with_root(airlock_root);
    paths.ensure_dirs().unwrap();

    let db = Database::open(&paths.database()).unwrap();
    let canonical_path = working_dir.canonicalize().unwrap();

    // Test 1: Create repo with a known last_sync timestamp (1 hour ago)
    let one_hour_ago = now_timestamp() - 3600;
    let test_repo = Repo {
        id: "test_sync".to_string(),
        working_path: canonical_path.clone(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: PathBuf::from("/tmp/gate.git"),
        last_sync: Some(one_hour_ago),
        created_at: now_timestamp() - 86400,
    };
    db.insert_repo(&test_repo).unwrap();

    // Verify the repo was stored with the correct last_sync
    let retrieved_repo = db.get_repo_by_path(&canonical_path).unwrap().unwrap();
    assert_eq!(
        retrieved_repo.last_sync,
        Some(one_hour_ago),
        "Repo last_sync should match the stored value"
    );

    // Run status - should succeed and display last sync info
    let result = run_with_paths(&working_dir, &paths);
    assert!(
        result.is_ok(),
        "Status command failed when last_sync is present: {:?}",
        result.err()
    );

    // Verify the format_time_ago function works for the stored timestamp
    let time_ago = format_time_ago(one_hour_ago);
    assert_eq!(
        time_ago, "1h ago",
        "Time ago formatting should show 1h ago for timestamp 1 hour in the past"
    );
}

/// E2E test verifying that `airlock status` handles repos that have never synced.
///
/// This test validates Section 7.3 of the test plan:
/// - "E2E: Shows last sync timestamp" (edge case: never synced)
///
/// The test creates a repo with last_sync = None and verifies:
/// 1. The status command succeeds when last_sync is None
/// 2. The output would show "never" instead of a timestamp
#[test]
fn test_e2e_status_shows_never_synced() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);
    repo.remote("origin", "https://github.com/user/repo.git")
        .unwrap();

    let paths = AirlockPaths::with_root(airlock_root);
    paths.ensure_dirs().unwrap();

    let db = Database::open(&paths.database()).unwrap();
    let canonical_path = working_dir.canonicalize().unwrap();

    // Create repo with NO last_sync (never synced)
    let test_repo = Repo {
        id: "test_never_synced".to_string(),
        working_path: canonical_path.clone(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: PathBuf::from("/tmp/gate.git"),
        last_sync: None, // Never synced
        created_at: now_timestamp(),
    };
    db.insert_repo(&test_repo).unwrap();

    // Verify the repo was stored with None for last_sync
    let retrieved_repo = db.get_repo_by_path(&canonical_path).unwrap().unwrap();
    assert!(
        retrieved_repo.last_sync.is_none(),
        "Repo last_sync should be None for never-synced repo"
    );

    // Run status - should succeed and display "never" for last sync
    let result = run_with_paths(&working_dir, &paths);
    assert!(
        result.is_ok(),
        "Status command failed when last_sync is None: {:?}",
        result.err()
    );

    // The status command output for never-synced repos (verified by code inspection at line 88):
    // "Last sync:  never"
    // This test verifies the command handles the None case without errors.
}

/// E2E test verifying that `airlock status` correctly formats various time ranges.
///
/// This test validates Section 7.3 of the test plan:
/// - "E2E: Shows last sync timestamp" (various time ranges)
///
/// Tests the format_time_ago function with various timestamps:
/// - Seconds ago
/// - Minutes ago
/// - Hours ago
/// - Days ago
/// - Months ago
#[test]
fn test_e2e_status_last_sync_various_time_ranges() {
    let now = now_timestamp();

    // Test various time ranges
    let test_cases = vec![
        (now - 5, "5s ago"),        // 5 seconds ago
        (now - 59, "59s ago"),      // 59 seconds ago
        (now - 60, "1m ago"),       // 1 minute ago
        (now - 300, "5m ago"),      // 5 minutes ago
        (now - 3599, "59m ago"),    // 59 minutes ago
        (now - 3600, "1h ago"),     // 1 hour ago
        (now - 7200, "2h ago"),     // 2 hours ago
        (now - 86399, "23h ago"),   // 23 hours ago
        (now - 86400, "1d ago"),    // 1 day ago
        (now - 172800, "2d ago"),   // 2 days ago
        (now - 2592000, "1mo ago"), // 30 days ago (1 month)
        (now - 31536000, "1y ago"), // 365 days ago (1 year)
    ];

    for (timestamp, expected) in test_cases {
        let result = format_time_ago(timestamp);
        assert_eq!(
            result, expected,
            "format_time_ago({}) should return '{}', got '{}'",
            timestamp, expected, result
        );
    }
}

/// E2E test verifying that `airlock status` shows daemon running status.
///
/// This test validates Section 7.3 of the test plan:
/// - "E2E: Shows daemon running status"
///
/// The implementation uses check_daemon_status() which:
/// 1. Checks if the daemon socket exists at paths.socket()
/// 2. Attempts to connect to verify daemon is responding
/// 3. Displays daemon status in the output ("Daemon: running ✓" or "Daemon: not running ✗")
#[test]
fn test_e2e_status_shows_daemon_running_status() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);
    repo.remote("origin", "https://github.com/user/repo.git")
        .unwrap();

    let paths = AirlockPaths::with_root(airlock_root);
    paths.ensure_dirs().unwrap();

    let db = Database::open(&paths.database()).unwrap();
    let canonical_path = working_dir.canonicalize().unwrap();

    let test_repo = Repo {
        id: "test_daemon_status".to_string(),
        working_path: canonical_path.clone(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: PathBuf::from("/tmp/gate.git"),
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };
    db.insert_repo(&test_repo).unwrap();

    // Run the status command - it should succeed and include daemon status
    let result = run_with_paths(&working_dir, &paths);
    assert!(
        result.is_ok(),
        "Status command should succeed: {:?}",
        result.err()
    );

    // Verify the check_daemon_status function works correctly
    // Since no daemon is running (no socket), it should return (false, "not running")
    let (daemon_running, daemon_message) = check_daemon_status(&paths);
    assert!(
        !daemon_running,
        "Daemon should not be running in test environment"
    );
    assert_eq!(
        daemon_message, "not running",
        "Message should indicate not running"
    );
}

/// E2E test verifying that `airlock status` shows daemon running when daemon is actually running.
///
/// This test validates Section 7.3 of the test plan:
/// - "E2E: Shows daemon running status"
#[cfg(unix)]
#[test]
fn test_e2e_status_shows_daemon_running_when_connected() {
    use std::os::unix::net::UnixListener;

    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);
    repo.remote("origin", "https://github.com/user/repo.git")
        .unwrap();

    let paths = AirlockPaths::with_root(airlock_root);
    paths.ensure_dirs().unwrap();

    let db = Database::open(&paths.database()).unwrap();
    let canonical_path = working_dir.canonicalize().unwrap();

    let test_repo = Repo {
        id: "test_daemon_running".to_string(),
        working_path: canonical_path.clone(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: PathBuf::from("/tmp/gate.git"),
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };
    db.insert_repo(&test_repo).unwrap();

    // Create a socket to simulate a running daemon
    let socket_path = paths.socket();
    let _listener = UnixListener::bind(&socket_path).unwrap();

    // Verify the check_daemon_status function detects the running daemon
    let (daemon_running, daemon_message) = check_daemon_status(&paths);
    assert!(daemon_running, "Daemon should be detected as running");
    assert_eq!(daemon_message, "running", "Message should indicate running");

    // Run the status command - it should succeed
    let result = run_with_paths(&working_dir, &paths);
    assert!(
        result.is_ok(),
        "Status command should succeed: {:?}",
        result.err()
    );
}

/// E2E test verifying that `airlock status` works when daemon is not running.
///
/// This test validates Section 7.3 of the test plan:
/// - "E2E: Works when daemon is not running (shows warning)"
///
/// The implementation:
/// - Works when daemon is down (reads from SQLite database)
/// - Shows warning "⚠ Warning: Daemon is not running. Some features may not work."
/// - Suggests running 'airlock daemon start'
#[test]
fn test_e2e_status_works_when_daemon_not_running() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();
    let repo = create_test_working_repo(&working_dir);
    repo.remote("origin", "https://github.com/user/repo.git")
        .unwrap();

    let paths = AirlockPaths::with_root(airlock_root);
    paths.ensure_dirs().unwrap();

    let db = Database::open(&paths.database()).unwrap();
    let canonical_path = working_dir.canonicalize().unwrap();

    let test_repo = Repo {
        id: "test_no_daemon".to_string(),
        working_path: canonical_path.clone(),
        upstream_url: "https://github.com/user/repo.git".to_string(),
        gate_path: PathBuf::from("/tmp/gate.git"),
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };
    db.insert_repo(&test_repo).unwrap();

    // IMPORTANT: We do NOT start any daemon here.
    // The daemon socket does NOT exist at paths.socket().
    // This simulates the "daemon not running" scenario.
    let socket_path = paths.socket();
    assert!(
        !socket_path.exists(),
        "Socket should not exist (no daemon running)"
    );

    // Run the status command - it should succeed even without daemon
    let result = run_with_paths(&working_dir, &paths);
    assert!(
        result.is_ok(),
        "Status command should work when daemon is not running: {:?}",
        result.err()
    );

    // Verify the check_daemon_status function returns false (daemon not running)
    let (daemon_running, daemon_message) = check_daemon_status(&paths);
    assert!(
        !daemon_running,
        "Daemon should not be running when socket doesn't exist"
    );
    assert_eq!(
        daemon_message, "not running",
        "Message should indicate daemon is not running"
    );

    // The implementation now shows a warning in the output when daemon is not running:
    // "⚠ Warning: Daemon is not running. Some features may not work."
    // "  Run 'airlock daemon start' to start the daemon."
    //
    // This is printed via println! in run_with_paths when daemon_running is false.
}

/// Test that check_daemon_status returns "not responding" when socket exists but nothing is listening.
#[cfg(unix)]
#[test]
fn test_check_daemon_status_not_responding() {
    use std::os::unix::net::UnixListener;

    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());
    paths.ensure_dirs().unwrap();

    // Create a socket file but don't have anything listening on it
    // by creating and immediately dropping the listener
    let socket_path = paths.socket();
    {
        let _listener = UnixListener::bind(&socket_path).unwrap();
        // Listener is dropped here, socket file remains but nothing is listening
    }

    // The socket file exists but no one is listening
    assert!(socket_path.exists(), "Socket file should exist");

    let (daemon_running, daemon_message) = check_daemon_status(&paths);
    assert!(
        !daemon_running,
        "Daemon should not be considered running when socket exists but not responding"
    );
    assert_eq!(
        daemon_message, "not responding",
        "Message should indicate daemon is not responding"
    );
}

/// E2E test verifying that `airlock status` in each repo shows correct information.
///
/// Status is derived from stage results. Verifies cross-repo isolation.
#[test]
fn test_e2e_status_in_each_repo_shows_correct_information() {
    const NUM_REPOS: usize = 3;

    let temp_dir = TempDir::new().unwrap();
    let airlock_root = temp_dir.path().join("airlock");
    let paths = AirlockPaths::with_root(airlock_root);
    paths.ensure_dirs().unwrap();

    let db = Database::open(&paths.database()).unwrap();

    struct RepoTestData {
        working_dir: PathBuf,
        repo_id: String,
        active_runs: usize,
        completed_runs: usize,
    }

    let mut repos_data: Vec<RepoTestData> = Vec::new();

    for i in 0..NUM_REPOS {
        let working_dir = temp_dir.path().join(format!("working_{}", i));
        fs::create_dir_all(&working_dir).unwrap();

        let repo = create_test_working_repo(&working_dir);
        repo.remote("origin", &format!("https://github.com/user/repo_{}.git", i))
            .unwrap();

        let canonical_path = working_dir.canonicalize().unwrap();
        let repo_id = format!("test_repo_{}", i);

        let test_repo = Repo {
            id: repo_id.clone(),
            working_path: canonical_path.clone(),
            upstream_url: format!("https://github.com/user/repo_{}.git", i),
            gate_path: PathBuf::from(format!("/tmp/gate_{}.git", i)),
            last_sync: if i == 0 {
                None
            } else {
                Some(now_timestamp() - (3600 * (i as i64)))
            },
            created_at: now_timestamp() - 86400,
        };
        db.insert_repo(&test_repo).unwrap();

        // Create active runs (with running stages)
        let active_count = i + 1; // 1, 2, 3
        for j in 0..active_count {
            let created = now_timestamp() - j as i64;
            let run = Run {
                id: format!("active_run_{}_{}", i, j),
                repo_id: repo_id.clone(),
                ref_updates: vec![],
                error: None,
                superseded: false,
                created_at: created,
                branch: String::new(),
                base_sha: String::new(),
                head_sha: String::new(),
                current_step: None,
                workflow_file: String::new(),
                workflow_name: None,
                updated_at: created,
            };
            db.insert_run(&run).unwrap();
            db.insert_job_result(&JobResult {
                id: format!("job_active_{}_{}", i, j),
                run_id: format!("active_run_{}_{}", i, j),
                job_key: "default".to_string(),
                name: Some("default".to_string()),
                status: JobStatus::Running,
                job_order: 0,
                started_at: None,
                completed_at: None,
                error: None,
            })
            .unwrap();
            db.insert_step_result(&StepResult {
                id: format!("sr_active_{}_{}", i, j),
                run_id: format!("active_run_{}_{}", i, j),
                job_id: format!("job_active_{}_{}", i, j),
                name: "test".to_string(),
                status: StepStatus::Running,
                step_order: 0,
                exit_code: None,
                duration_ms: None,
                error: None,
                started_at: None,
                completed_at: None,
            })
            .unwrap();
        }

        // Create completed runs (with passed stages)
        let completed_count = 2;
        for j in 0..completed_count {
            let created = now_timestamp() - 100 - j as i64;
            let run = Run {
                id: format!("done_run_{}_{}", i, j),
                repo_id: repo_id.clone(),
                ref_updates: vec![],
                error: None,
                superseded: false,
                created_at: created,
                branch: String::new(),
                base_sha: String::new(),
                head_sha: String::new(),
                current_step: None,
                workflow_file: String::new(),
                workflow_name: None,
                updated_at: created,
            };
            db.insert_run(&run).unwrap();
            db.insert_job_result(&JobResult {
                id: format!("job_done_{}_{}", i, j),
                run_id: format!("done_run_{}_{}", i, j),
                job_key: "default".to_string(),
                name: Some("default".to_string()),
                status: JobStatus::Passed,
                job_order: 0,
                started_at: None,
                completed_at: None,
                error: None,
            })
            .unwrap();
            db.insert_step_result(&StepResult {
                id: format!("sr_done_{}_{}", i, j),
                run_id: format!("done_run_{}_{}", i, j),
                job_id: format!("job_done_{}_{}", i, j),
                name: "test".to_string(),
                status: StepStatus::Passed,
                step_order: 0,
                exit_code: Some(0),
                duration_ms: None,
                error: None,
                started_at: None,
                completed_at: None,
            })
            .unwrap();
        }

        repos_data.push(RepoTestData {
            working_dir: canonical_path,
            repo_id,
            active_runs: active_count,
            completed_runs: completed_count,
        });
    }

    // Verify each repo sees only its own runs
    for (i, data) in repos_data.iter().enumerate() {
        let active_runs = db.list_active_runs(&data.repo_id).unwrap();
        assert_eq!(
            active_runs.len(),
            data.active_runs,
            "Repo {}: expected {} active runs",
            i,
            data.active_runs
        );

        // Verify isolation
        for run in &active_runs {
            assert_eq!(run.repo_id, data.repo_id);
        }

        let total = db.list_runs(&data.repo_id, Some(100)).unwrap();
        assert_eq!(total.len(), data.active_runs + data.completed_runs);

        let result = run_with_paths(&data.working_dir, &paths);
        assert!(
            result.is_ok(),
            "Repo {}: Status command failed: {:?}",
            i,
            result.err()
        );
    }
}
