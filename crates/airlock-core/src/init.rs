//! Shared init/eject logic for Airlock.
//!
//! This module contains the core logic for initializing and ejecting Airlock
//! from a repository. Both the CLI and daemon delegate to these functions,
//! differing only in error reporting and user-facing output.

use anyhow::{Context, Result};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

use crate::{git, jj, AirlockPaths, Database, Repo};

/// Workflows directory path relative to the working directory root.
pub const REPO_CONFIG_PATH: &str = ".airlock/workflows";

/// Default workflow filename created on init.
pub const DEFAULT_WORKFLOW_FILENAME: &str = "main.yml";

/// Default workflow content for new repositories.
pub const DEFAULT_WORKFLOW_YAML: &str = r#"# Airlock workflow configuration
# Documentation: https://github.com/airlock-hq/airlock

name: Main Pipeline

on:
  push:
    branches: ['**']

jobs:
  default:
    name: Lint, Test & Deploy
    steps:
      # Rebase onto upstream to handle drift
      - name: rebase
        uses: airlock-hq/airlock/defaults/rebase@main

      # Run linters and formatters, auto-fix issues
      - name: lint
        uses: airlock-hq/airlock/defaults/lint@main

      # Commit auto-fix patches and lock the worktree
      - name: freeze
        run: airlock exec freeze

      # Generate PR title and description from the diff
      - name: describe
        uses: airlock-hq/airlock/defaults/describe@main

      # Update documentation to reflect changes
      - name: document
        uses: airlock-hq/airlock/defaults/document@main

      # Run tests
      - name: test
        uses: airlock-hq/airlock/defaults/test@main

      # Push changes to upstream (pauses for user approval first)
      - name: push
        uses: airlock-hq/airlock/defaults/push@main
        require-approval: true

      # Create pull/merge request
      - name: create-pr
        uses: airlock-hq/airlock/defaults/create-pr@main
"#;

/// Result of a successful init operation.
pub struct InitOutcome {
    pub repo_id: String,
    pub gate_path: PathBuf,
    pub upstream_url: String,
    pub config_created: bool,
}

/// Result of a successful eject operation.
pub struct EjectOutcome {
    pub upstream_url: String,
}

/// Generate a repo ID from the origin URL and working path.
/// Uses a hash of the combined string for uniqueness.
pub fn generate_repo_id(origin_url: &str, working_path: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    origin_url.hash(&mut hasher);
    working_path.hash(&mut hasher);
    let hash = hasher.finish();

    // Use first 12 hex characters for a reasonably unique, readable ID
    format!("{:012x}", hash & 0xffffffffffff)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Initialize Airlock for the repository at `working_dir`.
///
/// This performs the full init sequence:
/// 1. Discover the git repo and validate state
/// 2. Create a bare repo "gate" with hooks
/// 3. Rewire remotes (origin → gate, upstream → original)
/// 4. Record in database and sync
///
/// On failure after mutations have started, best-effort rollback is performed.
pub fn init_repo(working_dir: &Path, paths: &AirlockPaths, db: &Database) -> Result<InitOutcome> {
    info!("Initializing Airlock in current repository...");

    // === Phase 1: Validation (no side effects) ===

    let working_repo = git::discover_repo(working_dir).context("Not inside a Git repository")?;

    let working_path = working_repo
        .workdir()
        .ok_or_else(|| anyhow::anyhow!("Cannot initialize Airlock in a bare repository"))?
        .to_path_buf()
        .canonicalize()
        .context("Failed to canonicalize working directory path")?;

    debug!("Working repository: {}", working_path.display());

    if !git::remote_exists(&working_repo, "origin") {
        anyhow::bail!(
            "No 'origin' remote found. Airlock requires an 'origin' remote to initialize.\n\
             Add one with: git remote add origin <url>"
        );
    }

    if git::remote_exists(&working_repo, "upstream") {
        anyhow::bail!(
            "An 'upstream' remote already exists. This repository may already be initialized with Airlock.\n\
             Run 'airlock eject' first if you want to re-initialize."
        );
    }

    let origin_url =
        git::get_remote_url(&working_repo, "origin").context("Failed to get origin URL")?;

    debug!("Origin URL: {}", origin_url);

    let repo_id = generate_repo_id(&origin_url, &working_path);
    debug!("Generated repo ID: {}", repo_id);

    let gate_path = paths.repo_gate(&repo_id);

    if gate_path.exists() {
        anyhow::bail!(
            "Gate repository already exists at {}. This might be a collision or stale data.\n\
             Remove it manually or run 'airlock eject' in the original repository.",
            gate_path.display()
        );
    }

    if let Some(existing) = db
        .get_repo_by_path(&working_path)
        .context("Failed to check for existing repo")?
    {
        anyhow::bail!(
            "This repository is already enrolled in Airlock (ID: {}).\n\
             Run 'airlock eject' first if you want to re-initialize.",
            existing.id
        );
    }

    // === Phase 2: Mutations (need cleanup on failure) ===

    paths
        .ensure_dirs()
        .context("Failed to create Airlock directories")?;

    let result = do_init(
        &working_repo,
        &working_path,
        paths,
        db,
        &origin_url,
        &repo_id,
        &gate_path,
    );

    if result.is_err() {
        // Best-effort rollback: if we renamed origin → upstream, undo it.
        // We know upstream didn't exist before (validated above), so if it
        // exists now, we created it.
        if git::remote_exists(&working_repo, "upstream") {
            let _ = git::remove_remote(&working_repo, "origin");
            let _ = git::rename_remote(&working_repo, "upstream", "origin");
        }
        let _ = std::fs::remove_dir_all(&gate_path);
    }

    result
}

/// Inner init logic, separated so the caller can do rollback on error.
fn do_init(
    working_repo: &git2::Repository,
    working_path: &Path,
    paths: &AirlockPaths,
    db: &Database,
    origin_url: &str,
    repo_id: &str,
    gate_path: &Path,
) -> Result<InitOutcome> {
    // Clean up any stale persistent worktree from a previous enrollment
    // (e.g., if the user ran eject + init and the worktree dir wasn't fully removed).
    let persistent_wt = paths.repo_worktree(repo_id);
    if persistent_wt.exists() {
        warn!(
            "Removing stale persistent worktree at {} during init",
            persistent_wt.display()
        );
        if let Err(e) = std::fs::remove_dir_all(&persistent_wt) {
            warn!("Failed to remove stale persistent worktree: {}", e);
        }
    }

    // Create the bare repo gate
    let gate_repo =
        git::create_bare_repo(gate_path).context("Failed to create bare repository (gate)")?;

    // Add origin remote to bare repo pointing to original origin (e.g. GitHub)
    git::add_remote(&gate_repo, "origin", origin_url)
        .context("Failed to add origin remote to gate")?;

    // Configure SSH credentials on the gate to match the working repo
    git::configure_gate_ssh(working_path, gate_path, origin_url)
        .context("Failed to configure SSH for gate")?;

    debug!("Created bare repo gate with origin remote");

    // Rewire working repo remotes
    git::rename_remote(working_repo, "origin", "upstream")
        .context("Failed to rename origin to upstream")?;

    let gate_url = gate_path.to_string_lossy().to_string();
    git::add_remote(working_repo, "origin", &gate_url)
        .context("Failed to add new origin pointing to gate")?;

    debug!("Rewired remotes: origin -> gate, upstream -> original remote");

    // Install hooks in bare repo
    git::install_hooks(gate_path).context("Failed to install hooks in gate")?;
    debug!("Installed pre-receive and post-receive hooks");

    // Install upload-pack wrapper and configure working repo to use it
    git::install_upload_pack_wrapper(paths).context("Failed to install upload-pack wrapper")?;
    git::configure_upload_pack(working_path, &paths.upload_pack_wrapper())
        .context("Failed to configure upload-pack for working repo")?;
    debug!("Installed upload-pack wrapper");

    // Record repo in database
    let repo = Repo {
        id: repo_id.to_string(),
        working_path: working_path.to_path_buf(),
        upstream_url: origin_url.to_string(),
        gate_path: gate_path.to_path_buf(),
        last_sync: None,
        created_at: now(),
    };

    db.insert_repo(&repo)
        .context("Failed to record repository in database")?;
    debug!("Recorded repo in database");

    // Create default .airlock/workflows/main.yml if it doesn't exist
    let workflows_dir = working_path.join(REPO_CONFIG_PATH);
    let workflow_path = workflows_dir.join(DEFAULT_WORKFLOW_FILENAME);
    let config_created = if !workflow_path.exists() {
        std::fs::create_dir_all(&workflows_dir)
            .context("Failed to create .airlock/workflows directory")?;
        std::fs::write(&workflow_path, DEFAULT_WORKFLOW_YAML)
            .context("Failed to create .airlock/workflows/main.yml")?;
        debug!("Created default .airlock/workflows/main.yml");
        true
    } else {
        debug!(".airlock/workflows/main.yml already exists, skipping creation");
        false
    };

    // Initial sync from origin
    info!("Syncing from origin...");
    match git::mirror_from_remote(gate_path, "origin") {
        Ok(()) => {
            debug!("Initial sync completed");

            db.update_repo_last_sync(repo_id, now())
                .context("Failed to update last sync timestamp")?;

            // Fetch from the gate into the working repo
            debug!("Fetching from gate into working repo...");
            if let Err(e) = git::fetch(working_path, "origin") {
                warn!("Failed to fetch from gate (non-fatal): {}", e);
            }
        }
        Err(e) => {
            warn!("Initial sync failed (this is OK if the remote is empty or requires authentication): {}", e);
        }
    }

    // Repoint existing local branches from upstream to origin (gate).
    // This runs outside the mirror_from_remote success block because even if
    // mirror/fetch failed, we should still fix tracking for branches that
    // already have matching origin/* refs.
    debug!("Repointing tracking branches from upstream to origin...");
    if let Err(e) = git::repoint_tracking_branches(working_path, "upstream", "origin") {
        warn!("Failed to repoint tracking branches (non-fatal): {}", e);
    }

    // Create local tracking branches for remote branches
    debug!("Creating local tracking branches...");
    if let Err(e) = git::create_local_tracking_branches(working_path, "origin") {
        warn!(
            "Failed to create local tracking branches (non-fatal): {}",
            e
        );
    }

    // Ensure all local branches have tracking set for origin.
    // This is a safety net that catches branches that lost tracking during
    // the remote rewiring (e.g., branches that weren't tracking upstream
    // and thus weren't repointed, but do have matching origin/* refs).
    if let Err(e) = git::ensure_tracking_for_existing_branches(working_path, "origin") {
        warn!("Failed to ensure branch tracking (non-fatal): {}", e);
    }

    // Synchronize jj bookmark tracking (if colocated repo)
    if jj::is_colocated(working_path) {
        if jj::is_available() {
            debug!("Detected jj colocated repo, synchronizing bookmarks...");
            if let Err(e) = jj::sync_after_init(working_path) {
                warn!("Failed to synchronize jj bookmarks (non-fatal): {}", e);
            }
        } else {
            warn!("jj colocated repo detected but jj not in PATH");
        }
    }

    info!(
        "Initialized Airlock for repo {} at {}",
        repo_id,
        working_path.display()
    );

    Ok(InitOutcome {
        repo_id: repo_id.to_string(),
        gate_path: gate_path.to_path_buf(),
        upstream_url: origin_url.to_string(),
        config_created,
    })
}

/// Eject Airlock from the repository at `working_dir`.
///
/// This reverses the init sequence:
/// 1. Look up the repo in the database
/// 2. Restore original remote configuration
/// 3. Remove gate, hooks, database record, and artifacts
pub fn eject_repo(working_dir: &Path, paths: &AirlockPaths, db: &Database) -> Result<EjectOutcome> {
    info!("Ejecting from Airlock...");

    let working_repo = git::discover_repo(working_dir).context("Not inside a Git repository")?;

    let working_path = working_repo
        .workdir()
        .ok_or_else(|| anyhow::anyhow!("Cannot eject from a bare repository"))?
        .to_path_buf()
        .canonicalize()
        .context("Failed to canonicalize working directory path")?;

    debug!("Working repository: {}", working_path.display());

    // Look up repo in database
    let repo = db
        .get_repo_by_path(&working_path)
        .context("Failed to query database")?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "This repository is not enrolled in Airlock.\n\
                 Run 'airlock init' first to initialize."
            )
        })?;

    debug!("Found repo in database: {}", repo.id);

    // Validate that upstream remote exists
    if !git::remote_exists(&working_repo, "upstream") {
        anyhow::bail!(
            "No 'upstream' remote found. The repository may be in an inconsistent state.\n\
             Please manually fix your remotes or remove the repo from the database."
        );
    }

    let upstream_url =
        git::get_remote_url(&working_repo, "upstream").context("Failed to get upstream URL")?;

    // Repoint branches from origin (gate) to upstream before removing origin.
    if let Err(e) = git::repoint_tracking_branches(&working_path, "origin", "upstream") {
        warn!("Failed to repoint branches before eject (non-fatal): {}", e);
    }

    // Remove origin (which points to the gate)
    if git::remote_exists(&working_repo, "origin") {
        git::remove_remote(&working_repo, "origin").context("Failed to remove origin remote")?;
        debug!("Removed origin remote (gate)");
    }

    // Rename upstream back to origin
    git::rename_remote(&working_repo, "upstream", "origin")
        .context("Failed to rename upstream to origin")?;
    debug!("Renamed upstream to origin");

    // Restore tracking for branches that may have lost it when origin was removed.
    // remove_remote strips branch.*.remote/merge config for branches tracking the deleted remote,
    // and repoint_tracking_branches may have failed if remote refs didn't exist yet.
    if let Err(e) = git::ensure_tracking_for_existing_branches(&working_path, "origin") {
        warn!(
            "Failed to ensure branch tracking after eject (non-fatal): {}",
            e
        );
    }

    // Synchronize jj bookmark tracking (if colocated repo)
    if jj::is_colocated(&working_path) {
        if jj::is_available() {
            debug!("Detected jj colocated repo, synchronizing bookmarks...");
            if let Err(e) = jj::sync_after_eject(&working_path) {
                warn!("Failed to synchronize jj bookmarks (non-fatal): {}", e);
            }
        } else {
            warn!("jj colocated repo detected but jj not in PATH");
        }
    }

    // Remove hooks from gate (if gate exists)
    if repo.gate_path.exists() {
        if let Err(e) = git::remove_hooks(&repo.gate_path) {
            warn!("Failed to remove hooks from gate: {}", e);
        } else {
            debug!("Removed hooks from gate");
        }
    }

    // Remove bare repo gate
    if repo.gate_path.exists() {
        std::fs::remove_dir_all(&repo.gate_path).context("Failed to remove gate repository")?;
        debug!("Removed gate repository: {}", repo.gate_path.display());
    }

    // Remove repo from database (cascades to runs, intents, sync_logs)
    db.delete_repo(&repo.id)
        .context("Failed to remove repo from database")?;
    debug!("Removed repo from database");

    // Clean up persistent worktree (must happen after gate removal)
    let worktree_dir = paths.repo_worktree(&repo.id);
    if worktree_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&worktree_dir) {
            warn!("Failed to remove persistent worktree: {}", e);
        } else {
            debug!("Removed persistent worktree: {}", worktree_dir.display());
        }
    }

    // Clean up artifacts
    let artifacts_path = paths.repo_artifacts(&repo.id);
    if artifacts_path.exists() {
        if let Err(e) = std::fs::remove_dir_all(&artifacts_path) {
            warn!("Failed to remove artifacts: {}", e);
        } else {
            debug!("Removed artifacts: {}", artifacts_path.display());
        }
    }

    info!("Ejected from Airlock: {}", working_path.display());

    Ok(EjectOutcome { upstream_url })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_repo_id() {
        let id1 = generate_repo_id("git@github.com:user/repo.git", Path::new("/home/user/repo"));
        let id2 = generate_repo_id("git@github.com:user/repo.git", Path::new("/home/user/repo"));
        let id3 = generate_repo_id(
            "git@github.com:user/other.git",
            Path::new("/home/user/repo"),
        );

        // Same inputs produce same ID
        assert_eq!(id1, id2);
        // Different inputs produce different ID
        assert_ne!(id1, id3);
        // ID is 12 hex characters
        assert_eq!(id1.len(), 12);
        assert!(id1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_generate_repo_id_deterministic_from_origin_and_path() {
        let origin_url_1 = "git@github.com:user/repo.git";
        let origin_url_2 = "https://github.com/user/repo.git";
        let path_1 = Path::new("/home/user/project-a");
        let path_2 = Path::new("/home/user/project-b");

        // Identical inputs always produce identical output
        let id_a1 = generate_repo_id(origin_url_1, path_1);
        let id_a2 = generate_repo_id(origin_url_1, path_1);
        let id_a3 = generate_repo_id(origin_url_1, path_1);
        assert_eq!(id_a1, id_a2);
        assert_eq!(id_a2, id_a3);

        // Different origin URL (same path) produces different ID
        let id_b = generate_repo_id(origin_url_2, path_1);
        assert_ne!(id_a1, id_b);

        // Different working path (same origin) produces different ID
        let id_c = generate_repo_id(origin_url_1, path_2);
        assert_ne!(id_a1, id_c);

        // Both different produces different ID
        let id_d = generate_repo_id(origin_url_2, path_2);
        assert_ne!(id_a1, id_d);
        assert_ne!(id_b, id_d);
        assert_ne!(id_c, id_d);

        // Order matters
        let id_e = generate_repo_id("https://example.com/foo", Path::new("/bar/baz"));
        let id_f = generate_repo_id("https://example.com/bar", Path::new("/foo/baz"));
        assert_ne!(id_e, id_f);

        // All generated IDs have correct format
        for id in [&id_a1, &id_b, &id_c, &id_d, &id_e, &id_f] {
            assert_eq!(id.len(), 12);
            assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        }

        // Edge cases: similar but not identical paths
        let id_similar_1 = generate_repo_id(origin_url_1, Path::new("/home/user/repo"));
        let id_similar_2 = generate_repo_id(origin_url_1, Path::new("/home/user/repo2"));
        let id_similar_3 = generate_repo_id(origin_url_1, Path::new("/home/user/repos"));
        assert_ne!(id_similar_1, id_similar_2);
        assert_ne!(id_similar_1, id_similar_3);
        assert_ne!(id_similar_2, id_similar_3);
    }
}
