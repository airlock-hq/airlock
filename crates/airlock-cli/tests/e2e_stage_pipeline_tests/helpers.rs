//! Shared test helpers for the e2e stage pipeline tests.

use airlock_core::{
    git, AirlockPaths, Database, JobResult, JobStatus, Repo, Run, StepResult, StepStatus,
};
use git2::{Repository, Signature};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

/// Get current Unix timestamp.
pub(super) fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Create a working repository with an initial commit and origin remote.
pub(super) fn create_working_repo(dir: &Path, origin_url: &str) -> Repository {
    let repo = Repository::init(dir).expect("Failed to init repo");

    // Create an initial commit
    {
        let sig = Signature::now("Test User", "test@example.com").unwrap();

        // Create a file
        let file_path = dir.join("README.md");
        fs::write(&file_path, "# Test Repository\n").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("README.md")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();
    }

    // Add origin remote
    repo.remote("origin", origin_url)
        .expect("Failed to add origin remote");

    repo
}

/// Create a bare repository to simulate upstream (GitHub).
pub(super) fn create_upstream_repo(dir: &Path) -> Repository {
    Repository::init_bare(dir).expect("Failed to init bare repo")
}

/// Set up a complete Airlock test environment.
/// Returns (temp_dir, paths, working_dir, repo_id, db).
pub(super) fn setup_airlock_env() -> (TempDir, AirlockPaths, std::path::PathBuf, String, Database) {
    let temp_dir = TempDir::new().unwrap();

    let upstream_dir = temp_dir.path().join("upstream.git");
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    // Create upstream bare repo
    create_upstream_repo(&upstream_dir);
    let upstream_url = upstream_dir.to_string_lossy().to_string();

    // Create working repo with origin pointing to upstream
    create_working_repo(&working_dir, &upstream_url);

    // Set up Airlock paths
    let paths = AirlockPaths::with_root(airlock_root);
    paths.ensure_dirs().unwrap();

    // Run init to set up gate
    let working_repo = git::discover_repo(&working_dir).unwrap();
    let working_path = working_repo
        .workdir()
        .unwrap()
        .to_path_buf()
        .canonicalize()
        .unwrap();

    let origin_url = git::get_remote_url(&working_repo, "origin").unwrap();

    // Generate repo ID
    let mut hasher = DefaultHasher::new();
    origin_url.hash(&mut hasher);
    working_path.hash(&mut hasher);
    let hash = hasher.finish();
    let repo_id = format!("{:012x}", hash & 0xffffffffffff);

    // Create gate bare repo
    let gate_path = paths.repo_gate(&repo_id);
    let gate_repo = git::create_bare_repo(&gate_path).unwrap();

    // Add origin remote to gate (points to GitHub)
    git::add_remote(&gate_repo, "origin", &origin_url).unwrap();

    // Rewire working repo remotes
    git::rename_remote(&working_repo, "origin", "upstream").unwrap();
    let gate_url = gate_path.to_string_lossy().to_string();
    git::add_remote(&working_repo, "origin", &gate_url).unwrap();

    // Install hooks
    git::install_hooks(&gate_path).unwrap();

    // Open database
    let db = Database::open(&paths.database()).unwrap();

    // Record repo in database
    let repo = Repo {
        id: repo_id.clone(),
        working_path: working_path.clone(),
        upstream_url: origin_url,
        gate_path,
        last_sync: Some(now_timestamp()),
        created_at: now_timestamp(),
    };
    db.insert_repo(&repo).unwrap();

    (temp_dir, paths, working_path, repo_id, db)
}

/// Create a test run in the database.
pub(super) fn create_test_run(db: &Database, repo_id: &str, branch: &str) -> String {
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
        base_sha: "0000000000000000000000000000000000000000".to_string(),
        head_sha: "abc123def456789012345678901234567890abcd".to_string(),
        current_step: None,
        workflow_file: "main.yml".to_string(),
        workflow_name: Some("Main Pipeline".to_string()),
        updated_at: created,
    };
    db.insert_run(&run).unwrap();
    run_id
}

/// Create a test job result in the database.
pub(super) fn create_test_job(
    db: &Database,
    run_id: &str,
    job_key: &str,
    status: JobStatus,
) -> String {
    let job_id = format!("job_{}", uuid::Uuid::new_v4());
    let result = JobResult {
        id: job_id.clone(),
        run_id: run_id.to_string(),
        job_key: job_key.to_string(),
        name: Some(job_key.to_string()),
        status,
        job_order: 0,
        started_at: if status != JobStatus::Pending {
            Some(now_timestamp())
        } else {
            None
        },
        completed_at: if status.is_final() {
            Some(now_timestamp())
        } else {
            None
        },
        error: if status == JobStatus::Failed {
            Some("Job failed".to_string())
        } else {
            None
        },
    };
    db.insert_job_result(&result).unwrap();
    job_id
}

/// Create a test step result in the database (requires a valid job_id).
pub(super) fn create_step_result(
    db: &Database,
    run_id: &str,
    job_id: &str,
    name: &str,
    status: StepStatus,
) -> String {
    let step_id = format!("step_{}", uuid::Uuid::new_v4());
    let result = StepResult {
        id: step_id.clone(),
        run_id: run_id.to_string(),
        job_id: job_id.to_string(),
        name: name.to_string(),
        status,
        step_order: 0,
        exit_code: if status == StepStatus::Passed {
            Some(0)
        } else {
            None
        },
        duration_ms: Some(100),
        error: if status == StepStatus::Failed {
            Some("Test error".to_string())
        } else {
            None
        },
        started_at: Some(now_timestamp()),
        completed_at: if status.is_final() {
            Some(now_timestamp())
        } else {
            None
        },
    };
    db.insert_step_result(&result).unwrap();
    step_id
}
