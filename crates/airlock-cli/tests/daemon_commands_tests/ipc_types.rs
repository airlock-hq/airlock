//! E2E tests validating that daemon IPC responses deserialize into shared `airlock_core::ipc` types.
//!
//! These tests start a real daemon, seed the database, send RPC requests, and verify
//! the responses can be deserialized into the canonical shared types from `airlock_core::ipc`.

use super::common::DaemonTestEnv;

#[cfg(unix)]
use super::common::send_rpc;

/// Seed a repo, run, job result, and step result into the database.
/// Returns `(repo_id, run_id, job_id, step_id)`.
fn seed_database(paths: &airlock_core::AirlockPaths) -> (String, String, String, String) {
    use airlock_core::{Database, JobResult, JobStatus, Repo, Run, StepResult, StepStatus};
    use std::path::PathBuf;

    let db = Database::open(&paths.database()).expect("Failed to open database");

    let repo_id = uuid::Uuid::new_v4().to_string();
    let run_id = uuid::Uuid::new_v4().to_string();
    let job_id = uuid::Uuid::new_v4().to_string();
    let step_id = uuid::Uuid::new_v4().to_string();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    db.insert_repo(&Repo {
        id: repo_id.clone(),
        working_path: PathBuf::from("/tmp/test-repo"),
        upstream_url: "https://github.com/test/repo.git".to_string(),
        gate_path: PathBuf::from("/tmp/test-gate.git"),
        last_sync: None,
        created_at: now,
    })
    .expect("Failed to insert repo");

    db.insert_run(&Run {
        id: run_id.clone(),
        repo_id: repo_id.clone(),
        ref_updates: vec![],
        branch: "main".to_string(),
        base_sha: "aaa1111".to_string(),
        head_sha: "bbb2222".to_string(),
        current_step: None,
        error: None,
        superseded: false,
        workflow_file: "default.yml".to_string(),
        workflow_name: Some("Default Pipeline".to_string()),
        created_at: now,
        updated_at: now,
    })
    .expect("Failed to insert run");

    db.insert_job_result(&JobResult {
        id: job_id.clone(),
        run_id: run_id.clone(),
        job_key: "lint".to_string(),
        name: Some("Lint Code".to_string()),
        status: JobStatus::Passed,
        job_order: 0,
        started_at: Some(now),
        completed_at: Some(now + 5),
        error: None,
        worktree_path: None,
    })
    .expect("Failed to insert job result");

    db.insert_step_result(&StepResult {
        id: step_id.clone(),
        run_id: run_id.clone(),
        job_id: job_id.clone(),
        name: "run-linter".to_string(),
        status: StepStatus::Passed,
        step_order: 0,
        exit_code: Some(0),
        duration_ms: Some(5000),
        error: None,
        started_at: Some(now),
        completed_at: Some(now + 5),
    })
    .expect("Failed to insert step result");

    (repo_id, run_id, job_id, step_id)
}

/// Test that `get_runs` returns data that deserializes into `Vec<RunInfo>`.
#[tokio::test]
async fn test_e2e_get_runs_returns_valid_shared_types() {
    let env = match DaemonTestEnv::setup() {
        Some(e) => e,
        None => {
            println!("Skipping test: airlockd binary not found. Run 'cargo build' first.");
            return;
        }
    };

    let (repo_id, run_id, _job_id, _step_id) = seed_database(&env.paths);

    let mut child = env.spawn_daemon("daemon");
    assert!(env.wait_for_socket().await, "Daemon did not start in time");

    #[cfg(unix)]
    let result = send_rpc(
        &env.paths,
        "get_runs",
        serde_json::json!({ "repo_id": repo_id }),
    )
    .await;

    #[cfg(unix)]
    {
        // Deserialize into the shared types from airlock_core::ipc
        let runs_value = result.get("runs").expect("Response missing 'runs' field");
        let runs: Vec<airlock_core::ipc::RunInfo> = serde_json::from_value(runs_value.clone())
            .expect("Failed to deserialize into Vec<RunInfo>");

        assert_eq!(runs.len(), 1, "Expected exactly 1 run");

        let run = &runs[0];
        assert_eq!(run.id, run_id);
        assert_eq!(run.status, "completed");
        assert_eq!(run.repo_id.as_deref(), Some(repo_id.as_str()));
        assert_eq!(run.branch.as_deref(), Some("main"));
        assert!(run.error.is_none(), "Expected no error on successful run");

        println!("Test passed: get_runs response deserializes into Vec<RunInfo>");
    }

    // Cleanup
    #[cfg(unix)]
    env.send_shutdown().await;
    let _ = child.kill();
    let _ = child.wait();
}

/// Test that `get_run_detail` returns data that deserializes into shared types.
#[tokio::test]
async fn test_e2e_get_run_detail_returns_valid_shared_types() {
    let env = match DaemonTestEnv::setup() {
        Some(e) => e,
        None => {
            println!("Skipping test: airlockd binary not found. Run 'cargo build' first.");
            return;
        }
    };

    let (repo_id, run_id, _job_id, _step_id) = seed_database(&env.paths);

    let mut child = env.spawn_daemon("daemon");
    assert!(env.wait_for_socket().await, "Daemon did not start in time");

    #[cfg(unix)]
    let result = send_rpc(
        &env.paths,
        "get_run_detail",
        serde_json::json!({ "run_id": run_id }),
    )
    .await;

    #[cfg(unix)]
    {
        // Validate run detail
        let run_value = result.get("run").expect("Response missing 'run' field");
        assert_eq!(run_value["id"].as_str().unwrap(), run_id);
        assert_eq!(run_value["repo_id"].as_str().unwrap(), repo_id);
        assert_eq!(run_value["status"].as_str().unwrap(), "completed");
        assert_eq!(run_value["branch"].as_str().unwrap(), "main");
        assert_eq!(run_value["base_sha"].as_str().unwrap(), "aaa1111");
        assert_eq!(run_value["head_sha"].as_str().unwrap(), "bbb2222");

        // Deserialize jobs into shared type
        let jobs_value = result.get("jobs").expect("Response missing 'jobs' field");
        let jobs: Vec<airlock_core::ipc::JobResultInfo> =
            serde_json::from_value(jobs_value.clone())
                .expect("Failed to deserialize into Vec<JobResultInfo>");

        assert_eq!(jobs.len(), 1, "Expected exactly 1 job");
        assert_eq!(jobs[0].job_key, "lint");
        assert_eq!(jobs[0].name.as_deref(), Some("Lint Code"));
        assert_eq!(jobs[0].status, "passed");
        assert!(jobs[0].error.is_none());

        // Deserialize step results into shared type
        let steps_value = result
            .get("step_results")
            .expect("Response missing 'step_results' field");
        let steps: Vec<airlock_core::ipc::StepResultInfo> =
            serde_json::from_value(steps_value.clone())
                .expect("Failed to deserialize into Vec<StepResultInfo>");

        assert_eq!(steps.len(), 1, "Expected exactly 1 step");
        assert_eq!(steps[0].step, "run-linter");
        assert_eq!(steps[0].status, "passed");
        assert_eq!(steps[0].exit_code, Some(0));
        assert!(steps[0].error.is_none());

        println!("Test passed: get_run_detail response deserializes into shared IPC types");
    }

    // Cleanup
    #[cfg(unix)]
    env.send_shutdown().await;
    let _ = child.kill();
    let _ = child.wait();
}

/// Test that `get_repos` returns data with valid repo fields.
#[tokio::test]
async fn test_e2e_get_repos_returns_valid_types() {
    let env = match DaemonTestEnv::setup() {
        Some(e) => e,
        None => {
            println!("Skipping test: airlockd binary not found. Run 'cargo build' first.");
            return;
        }
    };

    let (repo_id, _run_id, _job_id, _step_id) = seed_database(&env.paths);

    let mut child = env.spawn_daemon("daemon");
    assert!(env.wait_for_socket().await, "Daemon did not start in time");

    #[cfg(unix)]
    let result = send_rpc(&env.paths, "get_repos", serde_json::json!({})).await;

    #[cfg(unix)]
    {
        let repos_value = result.get("repos").expect("Response missing 'repos' field");
        let repos: Vec<serde_json::Value> =
            serde_json::from_value(repos_value.clone()).expect("Failed to parse repos array");

        assert_eq!(repos.len(), 1, "Expected exactly 1 repo");

        let repo = &repos[0];
        assert_eq!(repo["id"].as_str().unwrap(), repo_id);
        assert_eq!(repo["working_path"].as_str().unwrap(), "/tmp/test-repo");
        assert_eq!(
            repo["upstream_url"].as_str().unwrap(),
            "https://github.com/test/repo.git"
        );
        assert_eq!(repo["gate_path"].as_str().unwrap(), "/tmp/test-gate.git");
        assert!(repo["created_at"].as_i64().is_some());

        println!("Test passed: get_repos response contains valid repo fields");
    }

    // Cleanup
    #[cfg(unix)]
    env.send_shutdown().await;
    let _ = child.kill();
    let _ = child.wait();
}
