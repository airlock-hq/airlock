use super::*;
use airlock_core::{git, init, REPO_CONFIG_PATH};
use git2::Repository;
use std::fs;
use tempfile::TempDir;

/// Create a test working repository with an initial commit and origin remote.
fn create_test_working_repo(dir: &Path, origin_url: &str) -> Repository {
    let repo = Repository::init(dir).expect("Failed to init repo");

    // Create an initial commit
    {
        let sig = repo
            .signature()
            .unwrap_or_else(|_| git2::Signature::now("Test", "test@example.com").unwrap());

        let tree_id = {
            let mut index = repo.index().unwrap();
            index.write_tree().unwrap()
        };
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();
    }

    // Add origin remote
    repo.remote("origin", origin_url)
        .expect("Failed to add origin remote");

    repo
}

/// Create a bare repository to simulate a remote (like GitHub).
fn create_mock_remote(dir: &Path) -> Repository {
    Repository::init_bare(dir).expect("Failed to init bare repo")
}

#[test]
fn test_init_creates_gate_and_rewires_remotes() {
    // Set up temp directories
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    // Create mock remote (simulates GitHub)
    create_mock_remote(&remote_dir);
    let remote_url = remote_dir.to_string_lossy().to_string();

    // Create working repo with origin pointing to mock remote
    create_test_working_repo(&working_dir, &remote_url);

    // Set up Airlock paths in temp directory
    let paths = AirlockPaths::with_root(airlock_root.clone());

    // Run init
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    // Verify: Gate was created
    let repos_dir = paths.repos_dir();
    let gate_entries: Vec<_> = fs::read_dir(&repos_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(gate_entries.len(), 1, "Expected exactly one gate repo");

    let gate_path = gate_entries[0].path();
    assert!(gate_path.exists());
    assert!(gate_path.to_string_lossy().ends_with(".git"));

    // Verify: Gate is a bare repo with origin remote (pointing to GitHub)
    let gate_repo = Repository::open(&gate_path).unwrap();
    assert!(gate_repo.is_bare());
    let gate_origin = gate_repo.find_remote("origin").unwrap();
    assert_eq!(gate_origin.url().unwrap(), remote_url);

    // Verify: Hooks were installed
    assert!(gate_path.join("hooks/pre-receive").exists());
    assert!(gate_path.join("hooks/post-receive").exists());

    // Verify: Working repo remotes were rewired
    let working_repo = Repository::open(&working_dir).unwrap();

    // origin now points to gate
    let origin = working_repo.find_remote("origin").unwrap();
    assert_eq!(origin.url().unwrap(), gate_path.to_string_lossy());

    // bypass-airlock points to original remote
    let bypass = working_repo.find_remote("bypass-airlock").unwrap();
    assert_eq!(bypass.url().unwrap(), remote_url);

    // Verify: Repo was recorded in database
    let db = Database::open(&paths.database()).unwrap();
    let canonical_working = working_dir.canonicalize().unwrap();
    let repo_record = db.get_repo_by_path(&canonical_working).unwrap();
    assert!(repo_record.is_some(), "Repo should be in database");

    let repo_record = repo_record.unwrap();
    assert_eq!(repo_record.upstream_url, remote_url);
    assert_eq!(repo_record.gate_path, gate_path);
}

#[test]
fn test_init_fails_without_origin_remote() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    // Create repo WITHOUT origin remote
    let repo = Repository::init(&working_dir).unwrap();
    {
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])
            .unwrap();
    }

    let paths = AirlockPaths::with_root(airlock_root);
    let result = run_with_paths(&working_dir, &paths);

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("No 'origin' remote found"));
}

#[test]
fn test_init_fails_with_existing_upstream() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();
    create_mock_remote(&remote_dir);

    let remote_url = remote_dir.to_string_lossy().to_string();
    let repo = create_test_working_repo(&working_dir, &remote_url);

    // Add upstream remote (simulating already initialized)
    repo.remote("bypass-airlock", "https://example.com/other.git")
        .unwrap();

    let paths = AirlockPaths::with_root(airlock_root);
    let result = run_with_paths(&working_dir, &paths);

    assert!(result.is_err(), "Expected error but got success");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("'bypass-airlock' remote already exists"),
        "Unexpected error message: {}",
        err_msg
    );
}

#[test]
fn test_init_fails_on_bare_repo() {
    let temp_dir = TempDir::new().unwrap();
    let bare_dir = temp_dir.path().join("bare.git");
    let airlock_root = temp_dir.path().join("airlock");

    // Create a bare repo (not a working repo)
    Repository::init_bare(&bare_dir).unwrap();

    let paths = AirlockPaths::with_root(airlock_root);
    let result = run_with_paths(&bare_dir, &paths);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("bare repository"));
}

#[test]
fn test_init_fails_outside_git_repo() {
    let temp_dir = TempDir::new().unwrap();
    let airlock_root = temp_dir.path().join("airlock");

    // temp_dir.path() is not a git repo
    let paths = AirlockPaths::with_root(airlock_root);
    let result = run_with_paths(temp_dir.path(), &paths);

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Not inside a Git repository"));
}

#[test]
fn test_init_fails_if_already_enrolled() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();
    create_mock_remote(&remote_dir);

    let remote_url = remote_dir.to_string_lossy().to_string();
    create_test_working_repo(&working_dir, &remote_url);

    let paths = AirlockPaths::with_root(airlock_root.clone());

    // First init should succeed
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "First init should succeed");

    // Get the gate path that was created
    let gate_entries: Vec<_> = fs::read_dir(paths.repos_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    let gate_path = gate_entries[0].path();

    // Manually restore remotes AND delete gate to simulate cleanup
    let working_repo = Repository::open(&working_dir).unwrap();
    working_repo.remote_delete("origin").unwrap();
    working_repo
        .remote_rename("bypass-airlock", "origin")
        .unwrap();
    fs::remove_dir_all(&gate_path).unwrap();

    // Second init should fail (repo already in database)
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_err(), "Second init should fail");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("already enrolled"),
        "Unexpected error message: {}",
        err_msg
    );
}

#[test]
fn test_init_creates_bare_repo_gate_at_correct_path() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    create_mock_remote(&remote_dir);
    let remote_url = remote_dir.to_string_lossy().to_string();

    create_test_working_repo(&working_dir, &remote_url);

    let paths = AirlockPaths::with_root(airlock_root.clone());

    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    let repos_dir = paths.repos_dir();
    assert!(repos_dir.exists());
    assert_eq!(repos_dir, airlock_root.join("repos"));

    let gate_entries: Vec<_> = fs::read_dir(&repos_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(gate_entries.len(), 1);

    let gate_path = gate_entries[0].path();
    let gate_filename = gate_path.file_name().unwrap().to_string_lossy();

    assert!(gate_filename.ends_with(".git"));

    let repo_id = gate_filename.trim_end_matches(".git");
    assert_eq!(repo_id.len(), 12);
    assert!(repo_id.chars().all(|c| c.is_ascii_hexdigit()));

    let expected_gate_path = paths.repo_gate(repo_id);
    assert_eq!(gate_path, expected_gate_path);

    let gate_repo = Repository::open(&gate_path).expect("Should be able to open gate as repo");
    assert!(gate_repo.is_bare());

    assert!(gate_path.join("HEAD").exists());
    assert!(gate_path.join("objects").exists());
    assert!(gate_path.join("refs").exists());
}

#[test]
fn test_init_detects_https_origin_url_correctly() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    create_mock_remote(&remote_dir);

    let https_url = "https://github.com/testuser/testrepo.git";

    let repo = Repository::init(&working_dir).expect("Failed to init repo");
    {
        let sig = repo
            .signature()
            .unwrap_or_else(|_| git2::Signature::now("Test", "test@example.com").unwrap());

        let tree_id = {
            let mut index = repo.index().unwrap();
            index.write_tree().unwrap()
        };
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();
    }

    repo.remote("origin", https_url)
        .expect("Failed to add origin remote");

    let paths = AirlockPaths::with_root(airlock_root.clone());

    let _ = run_with_paths(&working_dir, &paths);

    let db = Database::open(&paths.database()).unwrap();
    let canonical_working = working_dir.canonicalize().unwrap();
    let repo_record = db.get_repo_by_path(&canonical_working).unwrap();

    if let Some(record) = repo_record {
        assert_eq!(record.upstream_url, https_url);
    }

    if paths.repos_dir().exists() {
        let gate_entries: Vec<_> = fs::read_dir(paths.repos_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        if !gate_entries.is_empty() {
            let gate_path = gate_entries[0].path();
            let gate_repo = Repository::open(&gate_path).unwrap();
            let gate_origin = gate_repo.find_remote("origin").unwrap();
            assert_eq!(gate_origin.url().unwrap(), https_url);
        }
    }
}

#[test]
fn test_init_detects_ssh_origin_url_correctly() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    let ssh_url = "git@github.com:testuser/testrepo.git";

    let repo = Repository::init(&working_dir).expect("Failed to init repo");
    {
        let sig = repo
            .signature()
            .unwrap_or_else(|_| git2::Signature::now("Test", "test@example.com").unwrap());

        let tree_id = {
            let mut index = repo.index().unwrap();
            index.write_tree().unwrap()
        };
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();
    }

    repo.remote("origin", ssh_url)
        .expect("Failed to add origin remote");

    let paths = AirlockPaths::with_root(airlock_root.clone());

    let _ = run_with_paths(&working_dir, &paths);

    let db = Database::open(&paths.database()).unwrap();
    let canonical_working = working_dir.canonicalize().unwrap();
    let repo_record = db.get_repo_by_path(&canonical_working).unwrap();

    if let Some(record) = repo_record {
        assert_eq!(record.upstream_url, ssh_url);
    }

    if paths.repos_dir().exists() {
        let gate_entries: Vec<_> = fs::read_dir(paths.repos_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        if !gate_entries.is_empty() {
            let gate_path = gate_entries[0].path();
            let gate_repo = Repository::open(&gate_path).unwrap();
            let gate_origin = gate_repo.find_remote("origin").unwrap();
            assert_eq!(gate_origin.url().unwrap(), ssh_url);
        }
    }
}

#[test]
fn test_init_renames_origin_to_upstream_preserving_url() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    create_mock_remote(&remote_dir);
    let original_origin_url = remote_dir.to_string_lossy().to_string();

    create_test_working_repo(&working_dir, &original_origin_url);

    let working_repo = Repository::open(&working_dir).unwrap();
    let origin_before = working_repo.find_remote("origin").unwrap();
    assert_eq!(origin_before.url().unwrap(), original_origin_url);
    assert!(working_repo.find_remote("bypass-airlock").is_err());
    drop(origin_before);
    drop(working_repo);

    let paths = AirlockPaths::with_root(airlock_root.clone());
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    let working_repo = Repository::open(&working_dir).unwrap();

    let bypass = working_repo
        .find_remote("bypass-airlock")
        .expect("bypass-airlock remote should exist after init");
    assert_eq!(bypass.url().unwrap(), original_origin_url);

    let origin_after = working_repo
        .find_remote("origin")
        .expect("origin should still exist after init");
    assert_ne!(origin_after.url().unwrap(), original_origin_url);

    let gate_url = origin_after.url().unwrap();
    assert!(std::path::Path::new(gate_url).exists());
}

#[test]
fn test_init_sets_new_origin_to_point_to_local_gate() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    create_mock_remote(&remote_dir);
    let remote_url = remote_dir.to_string_lossy().to_string();

    create_test_working_repo(&working_dir, &remote_url);

    let paths = AirlockPaths::with_root(airlock_root.clone());
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    let working_repo = Repository::open(&working_dir).unwrap();
    let origin = working_repo.find_remote("origin").unwrap();
    let origin_url = origin.url().expect("origin should have a URL");

    let origin_path = std::path::Path::new(origin_url);
    assert!(origin_path.is_absolute() || origin_url.starts_with('/'));
    assert!(origin_path.exists());

    let repos_dir = paths.repos_dir();
    assert!(origin_path.starts_with(&repos_dir));
    assert!(origin_url.ends_with(".git"));

    let gate_repo = Repository::open(origin_path).unwrap();
    assert!(gate_repo.is_bare());

    let gate_origin = gate_repo.find_remote("origin").unwrap();
    assert_eq!(gate_origin.url().unwrap(), remote_url);

    assert!(origin_path.join("hooks/pre-receive").exists());
    assert!(origin_path.join("hooks/post-receive").exists());
}

#[test]
fn test_init_installs_pre_receive_hook_in_bare_repo() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    create_mock_remote(&remote_dir);
    let remote_url = remote_dir.to_string_lossy().to_string();

    create_test_working_repo(&working_dir, &remote_url);

    let paths = AirlockPaths::with_root(airlock_root.clone());
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    let repos_dir = paths.repos_dir();
    let gate_entries: Vec<_> = fs::read_dir(&repos_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(gate_entries.len(), 1);
    let gate_path = gate_entries[0].path();

    let pre_receive_path = gate_path.join("hooks/pre-receive");
    assert!(pre_receive_path.exists());

    let hook_content = fs::read_to_string(&pre_receive_path).unwrap();
    assert_eq!(hook_content, git::hooks::pre_receive_hook());
    assert!(hook_content.starts_with("#!/bin/sh"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&pre_receive_path).unwrap();
        let mode = metadata.permissions().mode();
        assert!(mode & 0o111 != 0);
        assert_eq!(mode & 0o777, 0o755);
    }
}

#[test]
fn test_init_installs_post_receive_hook_in_bare_repo() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    create_mock_remote(&remote_dir);
    let remote_url = remote_dir.to_string_lossy().to_string();

    create_test_working_repo(&working_dir, &remote_url);

    let paths = AirlockPaths::with_root(airlock_root.clone());
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    let repos_dir = paths.repos_dir();
    let gate_entries: Vec<_> = fs::read_dir(&repos_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(gate_entries.len(), 1);
    let gate_path = gate_entries[0].path();

    let post_receive_path = gate_path.join("hooks/post-receive");
    assert!(post_receive_path.exists());

    let hook_content = fs::read_to_string(&post_receive_path).unwrap();
    assert_eq!(hook_content, git::hooks::post_receive_hook());
    assert!(hook_content.starts_with("#!/bin/sh"));
    assert!(hook_content.contains("SOCKET="));
    assert!(hook_content.contains("push_received"));
    assert!(hook_content.contains("nc -U"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&post_receive_path).unwrap();
        let mode = metadata.permissions().mode();
        assert!(mode & 0o111 != 0);
        assert_eq!(mode & 0o777, 0o755);
    }
}

#[test]
fn test_init_triggers_initial_sync_from_upstream() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    let remote_repo = create_mock_remote(&remote_dir);
    let remote_url = remote_dir.to_string_lossy().to_string();

    let staging_dir = temp_dir.path().join("staging");
    fs::create_dir_all(&staging_dir).unwrap();
    let staging_repo = Repository::init(&staging_dir).expect("Failed to init staging repo");

    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let tree_id = staging_repo.index().unwrap().write_tree().unwrap();
    let tree = staging_repo.find_tree(tree_id).unwrap();
    let commit_oid = staging_repo
        .commit(
            Some("HEAD"),
            &sig,
            &sig,
            "Initial upstream commit",
            &tree,
            &[],
        )
        .unwrap();

    staging_repo.remote("origin", &remote_url).unwrap();
    {
        let head = staging_repo.head().unwrap();
        let branch_name = head.shorthand().unwrap_or("master");

        let mut remote = staging_repo.find_remote("origin").unwrap();
        let refspec = format!("+refs/heads/{}:refs/heads/{}", branch_name, branch_name);
        remote.push(&[&refspec], None).unwrap();
    }

    let remote_refs = remote_repo.references().unwrap();
    let remote_ref_names: Vec<String> = remote_refs
        .filter_map(|r| r.ok())
        .filter_map(|r| r.name().map(String::from))
        .collect();
    assert!(!remote_ref_names.is_empty());

    create_test_working_repo(&working_dir, &remote_url);

    let time_before_init = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let paths = AirlockPaths::with_root(airlock_root.clone());
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    let time_after_init = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let gate_entries: Vec<_> = fs::read_dir(paths.repos_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(gate_entries.len(), 1);
    let gate_path = gate_entries[0].path();

    let gate_repo = Repository::open(&gate_path).unwrap();

    let gate_refs = gate_repo.references().unwrap();
    let gate_ref_names: Vec<String> = gate_refs
        .filter_map(|r| r.ok())
        .filter_map(|r| r.name().map(String::from))
        .collect();

    let has_mirrored_refs = gate_ref_names.iter().any(|r| r.starts_with("refs/heads/"));
    let can_find_commit = gate_repo.find_commit(commit_oid).is_ok();

    assert!(
        has_mirrored_refs || can_find_commit,
        "Gate should have refs from upstream after sync. Found refs: {:?}",
        gate_ref_names
    );

    let db = Database::open(&paths.database()).unwrap();
    let canonical_working = working_dir.canonicalize().unwrap();
    let repo_record = db
        .get_repo_by_path(&canonical_working)
        .unwrap()
        .expect("Repo should exist in database");

    assert!(repo_record.last_sync.is_some());
    let last_sync = repo_record.last_sync.unwrap();
    assert!(last_sync >= time_before_init && last_sync <= time_after_init);
}

#[test]
fn test_init_registers_repo_in_sqlite_database() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    create_mock_remote(&remote_dir);
    let remote_url = remote_dir.to_string_lossy().to_string();

    create_test_working_repo(&working_dir, &remote_url);

    let paths = AirlockPaths::with_root(airlock_root.clone());

    let time_before_init = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    let time_after_init = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let db_path = paths.database();
    assert!(db_path.exists());

    let db = Database::open(&db_path).unwrap();
    let canonical_working = working_dir.canonicalize().unwrap();

    let repo_by_path = db.get_repo_by_path(&canonical_working).unwrap();
    assert!(repo_by_path.is_some());
    let repo = repo_by_path.unwrap();

    assert_eq!(repo.id.len(), 12);
    assert!(repo.id.chars().all(|c| c.is_ascii_hexdigit()));

    let repo_by_id = db.get_repo(&repo.id).unwrap();
    assert!(repo_by_id.is_some());
    assert_eq!(repo.id, repo_by_id.unwrap().id);

    assert_eq!(repo.working_path, canonical_working);
    assert_eq!(repo.upstream_url, remote_url);
    assert!(repo.gate_path.exists());
    assert!(repo.gate_path.starts_with(paths.repos_dir()));
    assert!(repo.gate_path.to_string_lossy().ends_with(".git"));

    assert!(repo.created_at >= time_before_init && repo.created_at <= time_after_init);

    assert!(repo.last_sync.is_some());
    if let Some(last_sync) = repo.last_sync {
        assert!(last_sync >= time_before_init && last_sync <= time_after_init);
    }

    let all_repos = db.list_repos().unwrap();
    assert_eq!(all_repos.len(), 1);
    assert_eq!(all_repos[0].id, repo.id);
}

#[test]
fn test_init_creates_default_config() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    create_mock_remote(&remote_dir);
    let remote_url = remote_dir.to_string_lossy().to_string();

    create_test_working_repo(&working_dir, &remote_url);

    let workflows_dir = working_dir.join(REPO_CONFIG_PATH);
    let workflow_path = workflows_dir.join(init::DEFAULT_WORKFLOW_FILENAME);
    assert!(!workflows_dir.exists());

    let paths = AirlockPaths::with_root(airlock_root);
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    assert!(workflows_dir.exists());
    assert!(workflow_path.exists());

    let config_content = fs::read_to_string(&workflow_path).unwrap();
    assert!(config_content.contains("jobs:"));
    assert!(config_content.contains("steps:"));
    assert!(config_content.contains("name: lint"));
    assert!(config_content.contains("name: describe"));
    assert!(config_content.contains("name: test"));
    assert!(config_content.contains("name: critique"));
    assert!(config_content.contains("name: push"));
    assert!(config_content.contains("name: create-pr"));
    assert!(config_content.contains("name: review"));
    assert!(config_content.contains("airlock exec await"));
}

#[test]
fn test_init_overwrites_existing_config() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    create_mock_remote(&remote_dir);
    let remote_url = remote_dir.to_string_lossy().to_string();

    create_test_working_repo(&working_dir, &remote_url);

    // Pre-create the workflow file with custom content
    let workflows_dir = working_dir.join(REPO_CONFIG_PATH);
    let workflow_path = workflows_dir.join(init::DEFAULT_WORKFLOW_FILENAME);
    fs::create_dir_all(&workflows_dir).unwrap();
    let custom_config = "# Custom config\njobs:\n  default:\n    steps:\n      - name: custom-stage\n        run: echo custom\n";
    fs::write(&workflow_path, custom_config).unwrap();

    let paths = AirlockPaths::with_root(airlock_root);
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    // Init should overwrite with the default config
    let config_content = fs::read_to_string(&workflow_path).unwrap();
    assert!(!config_content.contains("custom-stage"));
    assert!(config_content.contains("name: describe"));
    assert!(config_content.contains("airlock exec await"));
}

#[test]
fn test_init_repoints_branches_from_upstream_to_origin() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    let _remote_repo = create_mock_remote(&remote_dir);
    let remote_url = remote_dir.to_string_lossy().to_string();

    let staging_dir = temp_dir.path().join("staging");
    fs::create_dir_all(&staging_dir).unwrap();
    let staging_repo = Repository::init(&staging_dir).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let tree_id = staging_repo.index().unwrap().write_tree().unwrap();
    let tree = staging_repo.find_tree(tree_id).unwrap();
    staging_repo
        .commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
        .unwrap();

    staging_repo.remote("origin", &remote_url).unwrap();
    let head = staging_repo.head().unwrap();
    let branch_name = head.shorthand().unwrap_or("master").to_string();
    drop(tree);
    drop(head);
    {
        let mut remote = staging_repo.find_remote("origin").unwrap();
        let refspec = format!("+refs/heads/{}:refs/heads/{}", branch_name, branch_name);
        remote.push(&[&refspec], None).unwrap();
    }
    drop(staging_repo);

    create_test_working_repo(&working_dir, &remote_url);

    let output = std::process::Command::new("git")
        .args(["-C", working_dir.to_str().unwrap()])
        .args(["fetch", "origin"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["-C", working_dir.to_str().unwrap()])
        .args([
            "branch",
            "--set-upstream-to",
            &format!("origin/{}", branch_name),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["-C", working_dir.to_str().unwrap()])
        .args([
            "for-each-ref",
            "--format=%(upstream:remotename)",
            "refs/heads/",
        ])
        .output()
        .unwrap();
    let tracking_before = String::from_utf8_lossy(&output.stdout);
    assert!(tracking_before.trim().contains("origin"));

    let paths = AirlockPaths::with_root(airlock_root);
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    let output = std::process::Command::new("git")
        .args(["-C", working_dir.to_str().unwrap()])
        .args([
            "for-each-ref",
            "--format=%(refname:short) %(upstream:remotename)",
            "refs/heads/",
        ])
        .output()
        .unwrap();
    let tracking_after = String::from_utf8_lossy(&output.stdout);

    for line in tracking_after.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        assert!(
            parts.len() == 2 && parts[1] == "origin",
            "Branch '{}' should track 'origin' but has tracking '{}'",
            parts[0],
            parts.get(1).unwrap_or(&"<none>")
        );
    }
}

/// E2E test: `airlock init` synchronizes jj bookmark tracking in a colocated repo.
#[test]
fn test_init_syncs_jj_bookmarks() {
    use airlock_core::jj;

    if !jj::is_available() {
        eprintln!("Skipping test: jj not installed");
        return;
    }

    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    create_mock_remote(&remote_dir);
    let remote_url = remote_dir.to_string_lossy().to_string();

    let staging_dir = temp_dir.path().join("staging");
    fs::create_dir_all(&staging_dir).unwrap();
    let staging_repo = Repository::init(&staging_dir).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let tree_id = staging_repo.index().unwrap().write_tree().unwrap();
    let tree = staging_repo.find_tree(tree_id).unwrap();
    staging_repo
        .commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
        .unwrap();
    staging_repo.remote("origin", &remote_url).unwrap();
    let head = staging_repo.head().unwrap();
    let branch_name = head.shorthand().unwrap_or("master").to_string();
    drop(tree);
    drop(head);
    {
        let mut remote = staging_repo.find_remote("origin").unwrap();
        let refspec = format!("+refs/heads/{}:refs/heads/{}", branch_name, branch_name);
        remote.push(&[&refspec], None).unwrap();
    }
    drop(staging_repo);

    let output = std::process::Command::new("jj")
        .args(["git", "init", "--colocate"])
        .current_dir(&working_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args([
            "-C",
            working_dir.to_str().unwrap(),
            "remote",
            "add",
            "origin",
            &remote_url,
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("git")
        .args(["-C", working_dir.to_str().unwrap(), "fetch", "origin"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = std::process::Command::new("jj")
        .args(["git", "import"])
        .current_dir(&working_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    assert!(jj::is_colocated(&working_dir));

    let paths = AirlockPaths::with_root(airlock_root);
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    let output = std::process::Command::new("jj")
        .args(["bookmark", "list", "-a"])
        .current_dir(&working_dir)
        .output()
        .unwrap();
    assert!(output.status.success());

    let bookmark_output = String::from_utf8_lossy(&output.stdout);
    assert!(
        bookmark_output.contains("@origin"),
        "After init, jj should show bookmarks tracking @origin. Got:\n{}",
        bookmark_output
    );
}

/// E2E test: eject → init round trip preserves branch tracking.
///
/// This tests the scenario where:
/// 1. User has a repo with tracking set up (simulating a clone)
/// 2. Run init → verify tracking points to origin (gate)
/// 3. Run eject → verify tracking points to origin (GitHub)
/// 4. Run init again → verify tracking points to origin (gate)
///
/// This was broken because eject's `remove_remote("origin")` stripped
/// branch tracking config, and re-init couldn't recover it.
#[test]
fn test_init_after_eject_preserves_tracking() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    // Create a mock remote with a commit
    let _remote_repo = Repository::init_bare(&remote_dir).unwrap();
    let remote_url = remote_dir.to_string_lossy().to_string();

    // Push a commit to the remote via a staging repo
    let staging_dir = temp_dir.path().join("staging");
    fs::create_dir_all(&staging_dir).unwrap();
    let staging_repo = Repository::init(&staging_dir).unwrap();
    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let tree_id = staging_repo.index().unwrap().write_tree().unwrap();
    let tree = staging_repo.find_tree(tree_id).unwrap();
    staging_repo
        .commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
        .unwrap();
    staging_repo.remote("origin", &remote_url).unwrap();
    let head = staging_repo.head().unwrap();
    let branch_name = head.shorthand().unwrap_or("master").to_string();
    drop(tree);
    drop(head);
    {
        let mut remote = staging_repo.find_remote("origin").unwrap();
        let refspec = format!("+refs/heads/{}:refs/heads/{}", branch_name, branch_name);
        remote.push(&[&refspec], None).unwrap();
    }
    drop(staging_repo);

    // Create working repo with tracking set up (like a git clone would)
    create_test_working_repo(&working_dir, &remote_url);
    let output = std::process::Command::new("git")
        .args(["-C", working_dir.to_str().unwrap()])
        .args(["fetch", "origin"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let output = std::process::Command::new("git")
        .args(["-C", working_dir.to_str().unwrap()])
        .args([
            "branch",
            "--set-upstream-to",
            &format!("origin/{}", branch_name),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let paths = AirlockPaths::with_root(airlock_root);

    // Run init
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "first init failed: {:?}", result.err());

    // After init: branch should track origin (gate)
    let tracking = get_branch_tracking(&working_dir);
    for line in tracking.lines() {
        let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
        if parts.len() == 2 && !parts[1].is_empty() {
            assert_eq!(
                parts[1], "origin",
                "After init, branch should track origin (gate)"
            );
        }
    }

    // Run eject
    let eject_result = super::super::eject::run_with_paths_for_test(&working_dir, &paths);
    assert!(
        eject_result.is_ok(),
        "eject failed: {:?}",
        eject_result.err()
    );

    // After eject: branch should track origin (real upstream)
    let tracking = get_branch_tracking(&working_dir);
    let mut found_tracking = false;
    for line in tracking.lines() {
        let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
        if parts.len() == 2 && !parts[1].is_empty() {
            found_tracking = true;
            assert_eq!(
                parts[1], "origin",
                "After eject, branch should track origin (upstream)"
            );
        }
    }
    assert!(
        found_tracking,
        "At least one branch should have tracking after eject"
    );

    // Run init again
    let result2 = run_with_paths(&working_dir, &paths);
    assert!(result2.is_ok(), "second init failed: {:?}", result2.err());

    // After re-init: branch should track origin (gate) again
    let tracking = get_branch_tracking(&working_dir);
    let mut found_tracking = false;
    for line in tracking.lines() {
        let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
        if parts.len() == 2 && !parts[1].is_empty() {
            found_tracking = true;
            assert_eq!(
                parts[1], "origin",
                "After re-init, branch should track origin (gate)"
            );
        }
    }
    assert!(
        found_tracking,
        "At least one branch should have tracking after re-init"
    );
}

/// E2E test: init sets tracking for branches that had no upstream.
///
/// When a repo was created with `git init` (not clone), branches may have
/// no tracking set. After `airlock init`, branches with matching origin/*
/// refs should get tracking.
#[test]
fn test_init_sets_tracking_for_untracked_branches() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();

    // Create a mock remote with a commit
    let _remote_repo = Repository::init_bare(&remote_dir).unwrap();
    let remote_url = remote_dir.to_string_lossy().to_string();

    // Create working repo and push WITHOUT -u (no tracking)
    create_test_working_repo(&working_dir, &remote_url);
    let working_repo = Repository::open(&working_dir).unwrap();
    let head = working_repo.head().unwrap();
    let branch_name = head.shorthand().unwrap_or("master").to_string();
    drop(head);
    drop(working_repo);

    // Push to remote without setting upstream
    let output = std::process::Command::new("git")
        .args(["-C", working_dir.to_str().unwrap()])
        .args(["push", "origin", &branch_name])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "push failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify: no tracking set
    let tracking = get_branch_tracking(&working_dir);
    for line in tracking.lines() {
        let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
        // Branch should have no tracking remote (empty second field)
        if parts.len() == 2 {
            assert!(
                parts[1].is_empty(),
                "Branch should have no tracking before init, but got '{}'",
                parts[1]
            );
        }
    }

    let paths = AirlockPaths::with_root(airlock_root);

    // Run init
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    // After init: branch should now track origin (gate)
    let tracking = get_branch_tracking(&working_dir);
    let mut found_tracking = false;
    for line in tracking.lines() {
        let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
        if parts.len() == 2 && !parts[1].is_empty() {
            found_tracking = true;
            assert_eq!(parts[1], "origin", "After init, branch should track origin");
        }
    }
    assert!(
        found_tracking,
        "At least one branch should have tracking set after init. Tracking output: {}",
        tracking
    );
}

/// Helper: get branch tracking info for all local branches.
fn get_branch_tracking(working_dir: &Path) -> String {
    let output = std::process::Command::new("git")
        .args(["-C", working_dir.to_str().unwrap()])
        .args([
            "for-each-ref",
            "--format=%(refname:short) %(upstream:remotename)",
            "refs/heads/",
        ])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn test_init_installs_upload_pack_wrapper() {
    let temp_dir = TempDir::new().unwrap();
    let working_dir = temp_dir.path().join("working");
    let remote_dir = temp_dir.path().join("remote.git");
    let airlock_root = temp_dir.path().join("airlock");

    fs::create_dir_all(&working_dir).unwrap();
    create_mock_remote(&remote_dir);
    let remote_url = remote_dir.to_string_lossy().to_string();
    create_test_working_repo(&working_dir, &remote_url);

    let paths = AirlockPaths::with_root(airlock_root);
    let result = run_with_paths(&working_dir, &paths);
    assert!(result.is_ok(), "init failed: {:?}", result.err());

    let wrapper_path = paths.upload_pack_wrapper();
    assert!(wrapper_path.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(&wrapper_path).unwrap();
        let mode = metadata.permissions().mode();
        assert!(mode & 0o111 != 0);
    }

    let wrapper_content = fs::read_to_string(&wrapper_path).unwrap();
    assert!(wrapper_content.contains("git-upload-pack"));

    let output = std::process::Command::new("git")
        .args(["-C", working_dir.to_str().unwrap()])
        .args(["config", "remote.origin.uploadpack"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let configured_path = String::from_utf8_lossy(&output.stdout);
    assert_eq!(configured_path.trim(), wrapper_path.to_string_lossy());
}
