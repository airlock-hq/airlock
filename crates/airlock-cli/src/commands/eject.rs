//! `airlock eject` command implementation.

use anyhow::{Context, Result};
use std::env;
use std::path::Path;
use tracing::{debug, warn};

use airlock_core::{git, init, AirlockPaths, Database};

/// Run the eject command to remove Airlock from the current repository.
pub async fn run() -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let paths = AirlockPaths::new().context("Failed to initialize Airlock paths")?;

    run_with_paths(&current_dir, &paths)
}

/// Internal implementation that accepts paths for testability.
/// Exposed as `pub(crate)` via `run_with_paths_for_test` for cross-module tests.
fn run_with_paths(working_dir: &Path, paths: &AirlockPaths) -> Result<()> {
    // Pre-check for inconsistent state: repo has 'upstream' remote but isn't in the database.
    // This can happen if init partially completed or the database was reset.
    let working_repo = git::discover_repo(working_dir).context("Not inside a Git repository")?;

    let working_path = working_repo
        .workdir()
        .ok_or_else(|| anyhow::anyhow!("Cannot eject from a bare repository"))?
        .to_path_buf()
        .canonicalize()
        .context("Failed to canonicalize working directory path")?;

    let db = Database::open(&paths.database()).context("Failed to open Airlock database")?;

    let repo = db
        .get_repo_by_path(&working_path)
        .context("Failed to query database")?;

    if repo.is_none() && git::remote_exists(&working_repo, "upstream") {
        warn!("Repository has 'upstream' remote but is not enrolled in database. Cleaning up...");

        let upstream_url =
            git::get_remote_url(&working_repo, "upstream").context("Failed to get upstream URL")?;

        // Remove origin (which might point to a non-existent gate)
        if git::remote_exists(&working_repo, "origin") {
            git::remove_remote(&working_repo, "origin")
                .context("Failed to remove origin remote")?;
            debug!("Removed origin remote");
        }

        // Rename upstream back to origin
        git::rename_remote(&working_repo, "upstream", "origin")
            .context("Failed to rename upstream to origin")?;
        debug!("Renamed upstream to origin");

        println!("Cleaned up inconsistent Airlock state.");
        println!();
        println!("Your remotes have been restored:");
        println!("  origin -> {}", upstream_url);
        println!();
        println!("You can now run 'airlock init' to re-initialize or push directly.");

        return Ok(());
    }

    // Normal eject path via shared core logic
    drop(working_repo);
    let outcome = init::eject_repo(working_dir, paths, &db)?;

    println!("Ejected from Airlock successfully.");
    println!();
    println!("Your remotes have been restored:");
    println!("  origin -> {}", outcome.upstream_url);
    println!();
    println!("You can now push directly with `git push origin <branch>`.");

    Ok(())
}

/// Exposed for cross-module testing (e.g., init tests that need to eject).
#[cfg(test)]
pub(crate) fn run_with_paths_for_test(working_dir: &Path, paths: &AirlockPaths) -> Result<()> {
    run_with_paths(working_dir, paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use airlock_core::Repo;
    use git2::Repository;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
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

    /// Create a staging repo, make an initial commit, and push it to the remote.
    /// Returns the branch name of the pushed commit.
    fn push_initial_commit_to_remote(temp_dir: &Path, remote_url: &str) -> String {
        let staging_dir = temp_dir.join("staging");
        fs::create_dir_all(&staging_dir).unwrap();
        let staging_repo = Repository::init(&staging_dir).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = staging_repo.index().unwrap().write_tree().unwrap();
        let tree = staging_repo.find_tree(tree_id).unwrap();
        staging_repo
            .commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();
        staging_repo.remote("origin", remote_url).unwrap();
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
        branch_name
    }

    /// Simulate the state after `airlock init` has been run.
    fn setup_initialized_repo(
        working_dir: &Path,
        remote_dir: &Path,
        paths: &AirlockPaths,
    ) -> (Repository, String) {
        // Create mock remote (simulates GitHub)
        create_mock_remote(remote_dir);
        let remote_url = remote_dir.to_string_lossy().to_string();

        // Create working repo with origin pointing to mock remote
        let working_repo = create_test_working_repo(working_dir, &remote_url);

        // Generate repo ID (same logic as init)
        let canonical_path = working_dir.canonicalize().unwrap();
        let repo_id = init::generate_repo_id(&remote_url, &canonical_path);

        // Create gate
        paths.ensure_dirs().unwrap();
        let gate_path = paths.repo_gate(&repo_id);
        let gate_repo = git::create_bare_repo(&gate_path).unwrap();
        git::add_remote(&gate_repo, "upstream", &remote_url).unwrap();
        git::install_hooks(&gate_path).unwrap();

        // Rewire working repo remotes (simulate init)
        git::rename_remote(&working_repo, "origin", "upstream").unwrap();
        let gate_url = gate_path.to_string_lossy().to_string();
        git::add_remote(&working_repo, "origin", &gate_url).unwrap();

        // Add to database
        let db = Database::open(&paths.database()).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let repo = Repo {
            id: repo_id.clone(),
            working_path: canonical_path,
            upstream_url: remote_url.clone(),
            gate_path,
            last_sync: Some(now),
            created_at: now,
        };
        db.insert_repo(&repo).unwrap();

        (working_repo, repo_id)
    }

    #[test]
    fn test_eject_restores_remotes_and_cleans_up() {
        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().join("working");
        let remote_dir = temp_dir.path().join("remote.git");
        let airlock_root = temp_dir.path().join("airlock");

        fs::create_dir_all(&working_dir).unwrap();

        let paths = AirlockPaths::with_root(airlock_root.clone());

        let (_working_repo, repo_id) = setup_initialized_repo(&working_dir, &remote_dir, &paths);

        let gate_path = paths.repo_gate(&repo_id);
        assert!(gate_path.exists());

        let db = Database::open(&paths.database()).unwrap();
        let canonical_path = working_dir.canonicalize().unwrap();
        assert!(db.get_repo_by_path(&canonical_path).unwrap().is_some());

        let pre_repo = Repository::open(&working_dir).unwrap();
        assert!(git::remote_exists(&pre_repo, "upstream"));

        let result = run_with_paths(&working_dir, &paths);
        assert!(result.is_ok(), "eject failed: {:?}", result.err());

        assert!(!gate_path.exists());

        let db = Database::open(&paths.database()).unwrap();
        assert!(db.get_repo_by_path(&canonical_path).unwrap().is_none());

        let post_repo = Repository::open(&working_dir).unwrap();

        let origin = post_repo.find_remote("origin").unwrap();
        let remote_url = remote_dir.to_string_lossy().to_string();
        assert_eq!(origin.url().unwrap(), remote_url);

        assert!(!git::remote_exists(&post_repo, "upstream"));
    }

    #[test]
    fn test_eject_cleans_up_artifacts() {
        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().join("working");
        let remote_dir = temp_dir.path().join("remote.git");
        let airlock_root = temp_dir.path().join("airlock");

        fs::create_dir_all(&working_dir).unwrap();

        let paths = AirlockPaths::with_root(airlock_root.clone());

        let (_working_repo, repo_id) = setup_initialized_repo(&working_dir, &remote_dir, &paths);

        let artifacts_path = paths.repo_artifacts(&repo_id);
        fs::create_dir_all(&artifacts_path).unwrap();
        fs::write(artifacts_path.join("test.json"), "{}").unwrap();
        assert!(artifacts_path.exists());

        let result = run_with_paths(&working_dir, &paths);
        assert!(result.is_ok(), "eject failed: {:?}", result.err());

        assert!(!artifacts_path.exists());
    }

    #[test]
    fn test_eject_fails_if_not_enrolled() {
        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().join("working");
        let remote_dir = temp_dir.path().join("remote.git");
        let airlock_root = temp_dir.path().join("airlock");

        fs::create_dir_all(&working_dir).unwrap();

        create_mock_remote(&remote_dir);
        let remote_url = remote_dir.to_string_lossy().to_string();
        create_test_working_repo(&working_dir, &remote_url);

        let paths = AirlockPaths::with_root(airlock_root);
        paths.ensure_dirs().unwrap();

        let _ = Database::open(&paths.database()).unwrap();

        let result = run_with_paths(&working_dir, &paths);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not enrolled"),
            "Error should mention not enrolled: {}",
            err_msg
        );
    }

    #[test]
    fn test_eject_fails_outside_git_repo() {
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
    fn test_eject_fails_on_bare_repo() {
        let temp_dir = TempDir::new().unwrap();
        let bare_dir = temp_dir.path().join("bare.git");
        let airlock_root = temp_dir.path().join("airlock");

        Repository::init_bare(&bare_dir).unwrap();

        let paths = AirlockPaths::with_root(airlock_root);
        let result = run_with_paths(&bare_dir, &paths);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("bare repository"));
    }

    #[test]
    fn test_eject_fails_without_upstream_remote() {
        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().join("working");
        let remote_dir = temp_dir.path().join("remote.git");
        let airlock_root = temp_dir.path().join("airlock");

        fs::create_dir_all(&working_dir).unwrap();

        let paths = AirlockPaths::with_root(airlock_root.clone());

        let (working_repo, _repo_id) = setup_initialized_repo(&working_dir, &remote_dir, &paths);

        // Manually remove upstream remote to simulate inconsistent state
        working_repo.remote_delete("upstream").unwrap();

        let result = run_with_paths(&working_dir, &paths);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("No 'upstream' remote found"),
            "Error should mention missing upstream: {}",
            err_msg
        );
    }

    #[test]
    fn test_eject_handles_missing_gate_gracefully() {
        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().join("working");
        let remote_dir = temp_dir.path().join("remote.git");
        let airlock_root = temp_dir.path().join("airlock");

        fs::create_dir_all(&working_dir).unwrap();

        let paths = AirlockPaths::with_root(airlock_root.clone());

        let (_working_repo, repo_id) = setup_initialized_repo(&working_dir, &remote_dir, &paths);

        let gate_path = paths.repo_gate(&repo_id);
        fs::remove_dir_all(&gate_path).unwrap();

        let result = run_with_paths(&working_dir, &paths);
        assert!(
            result.is_ok(),
            "eject should succeed even if gate is missing: {:?}",
            result.err()
        );
    }

    /// E2E Test: After eject, `git push origin` goes to original upstream.
    #[test]
    fn test_eject_push_origin_goes_to_original_upstream() {
        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().join("working");
        let remote_dir = temp_dir.path().join("remote.git");
        let airlock_root = temp_dir.path().join("airlock");

        fs::create_dir_all(&working_dir).unwrap();

        let paths = AirlockPaths::with_root(airlock_root.clone());

        let (_working_repo, _repo_id) = setup_initialized_repo(&working_dir, &remote_dir, &paths);

        let result = run_with_paths(&working_dir, &paths);
        assert!(result.is_ok(), "eject failed: {:?}", result.err());

        let working_repo = Repository::open(&working_dir).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();

        let file_path = working_dir.join("after_eject.txt");
        fs::write(&file_path, "This file was created after eject\n").unwrap();

        {
            let mut index = working_repo.index().unwrap();
            index.add_path(Path::new("after_eject.txt")).unwrap();
            index.write().unwrap();

            let tree_id = index.write_tree().unwrap();
            let tree = working_repo.find_tree(tree_id).unwrap();
            let head = working_repo.head().unwrap();
            let parent = head.peel_to_commit().unwrap();
            working_repo
                .commit(
                    Some("HEAD"),
                    &sig,
                    &sig,
                    "Add file after eject",
                    &tree,
                    &[&parent],
                )
                .unwrap();
        }

        {
            let mut remote = working_repo.find_remote("origin").unwrap();
            let result = remote.push(&["refs/heads/master:refs/heads/master"], None);
            assert!(
                result.is_ok(),
                "Push should succeed after eject: {:?}",
                result.err()
            );
        }

        let upstream_repo = Repository::open_bare(&remote_dir).unwrap();
        let upstream_head = upstream_repo
            .find_reference("refs/heads/master")
            .expect("Upstream should have master branch after push");
        let upstream_commit = upstream_head.peel_to_commit().unwrap();

        assert_eq!(upstream_commit.message().unwrap(), "Add file after eject");

        let gate_path = paths.repo_gate(&_repo_id);
        assert!(!gate_path.exists());
    }

    #[test]
    fn test_eject_cleans_up_inconsistent_state() {
        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().join("working");
        let remote_dir = temp_dir.path().join("remote.git");
        let airlock_root = temp_dir.path().join("airlock");

        fs::create_dir_all(&working_dir).unwrap();

        create_mock_remote(&remote_dir);
        let remote_url = remote_dir.to_string_lossy().to_string();

        let working_repo = create_test_working_repo(&working_dir, &remote_url);

        // Manually set up inconsistent state: rename origin to upstream
        git::rename_remote(&working_repo, "origin", "upstream").unwrap();

        assert!(git::remote_exists(&working_repo, "upstream"));
        assert!(!git::remote_exists(&working_repo, "origin"));

        let paths = AirlockPaths::with_root(airlock_root);
        paths.ensure_dirs().unwrap();

        let db = Database::open(&paths.database()).unwrap();
        let canonical_path = working_dir.canonicalize().unwrap();
        assert!(db.get_repo_by_path(&canonical_path).unwrap().is_none());

        let result = run_with_paths(&working_dir, &paths);
        assert!(
            result.is_ok(),
            "eject should succeed for inconsistent state: {:?}",
            result.err()
        );

        let post_repo = Repository::open(&working_dir).unwrap();
        assert!(git::remote_exists(&post_repo, "origin"));
        assert!(!git::remote_exists(&post_repo, "upstream"));

        let origin = post_repo.find_remote("origin").unwrap();
        assert_eq!(origin.url().unwrap(), remote_url);
    }

    #[test]
    fn test_eject_preserves_branch_tracking_after_init() {
        let temp_dir = TempDir::new().unwrap();
        let working_dir = temp_dir.path().join("working");
        let remote_dir = temp_dir.path().join("remote.git");
        let airlock_root = temp_dir.path().join("airlock");

        fs::create_dir_all(&working_dir).unwrap();

        let _remote_repo = create_mock_remote(&remote_dir);
        let remote_url = remote_dir.to_string_lossy().to_string();
        let branch_name = push_initial_commit_to_remote(temp_dir.path(), &remote_url);

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

        // Run init (uses the actual init code path)
        let init_result = super::super::init::run_with_paths(&working_dir, &paths);
        assert!(init_result.is_ok(), "init failed: {:?}", init_result.err());

        // After init: branch should track origin (gate)
        let output = std::process::Command::new("git")
            .args(["-C", working_dir.to_str().unwrap()])
            .args([
                "for-each-ref",
                "--format=%(refname:short) %(upstream:remotename)",
                "refs/heads/",
            ])
            .output()
            .unwrap();
        let tracking = String::from_utf8_lossy(&output.stdout);
        for line in tracking.lines() {
            let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
            if parts.len() == 2 && !parts[1].is_empty() {
                assert_eq!(parts[1], "origin");
            }
        }

        // Run eject
        let eject_result = run_with_paths(&working_dir, &paths);
        assert!(
            eject_result.is_ok(),
            "eject failed: {:?}",
            eject_result.err()
        );

        // After eject: branch should still track origin (now real upstream)
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

        let mut found_tracking = false;
        for line in tracking_after.lines() {
            let parts: Vec<&str> = line.trim().splitn(2, ' ').collect();
            if parts.len() == 2 && !parts[1].is_empty() {
                found_tracking = true;
                assert_eq!(parts[1], "origin");
            }
        }
        assert!(
            found_tracking,
            "At least one branch should have tracking set after eject. Output: {}",
            tracking_after
        );

        let post_repo = Repository::open(&working_dir).unwrap();
        let origin = post_repo.find_remote("origin").unwrap();
        assert_eq!(origin.url().unwrap(), remote_url);
    }

    /// E2E test: init -> eject round trip preserves jj bookmark tracking.
    #[test]
    fn test_init_eject_round_trip_with_jj() {
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
        push_initial_commit_to_remote(temp_dir.path(), &remote_url);

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

        let paths = AirlockPaths::with_root(airlock_root);

        // Run init via real code path
        let init_result = super::super::init::run_with_paths(&working_dir, &paths);
        assert!(init_result.is_ok(), "init failed: {:?}", init_result.err());

        let output = std::process::Command::new("jj")
            .args(["bookmark", "list", "-a"])
            .current_dir(&working_dir)
            .output()
            .unwrap();
        assert!(output.status.success());
        let bookmarks_after_init = String::from_utf8_lossy(&output.stdout);
        assert!(bookmarks_after_init.contains("@origin"));

        // Run eject via real code path
        let eject_result = run_with_paths(&working_dir, &paths);
        assert!(
            eject_result.is_ok(),
            "eject failed: {:?}",
            eject_result.err()
        );

        let output = std::process::Command::new("jj")
            .args(["bookmark", "list", "-a"])
            .current_dir(&working_dir)
            .output()
            .unwrap();
        assert!(output.status.success());
        let bookmarks_after_eject = String::from_utf8_lossy(&output.stdout);
        assert!(bookmarks_after_eject.contains("@origin"));

        let post_repo = Repository::open(&working_dir).unwrap();
        let origin = post_repo.find_remote("origin").unwrap();
        assert_eq!(origin.url().unwrap(), remote_url);
        assert!(!git::remote_exists(&post_repo, "upstream"));
    }
}
