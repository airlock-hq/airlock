//! Remote management operations for Airlock.

use crate::error::{AirlockError, Result};
use git2::Repository;

/// Get the URL of a remote.
pub fn get_remote_url(repo: &Repository, name: &str) -> Result<String> {
    let remote = repo.find_remote(name)?;
    let url = remote
        .url()
        .ok_or_else(|| AirlockError::Git(format!("Remote '{}' has no URL", name)))?;
    Ok(url.to_string())
}

/// Add a new remote to the repository.
pub fn add_remote(repo: &Repository, name: &str, url: &str) -> Result<()> {
    repo.remote(name, url)?;
    tracing::debug!("Added remote '{}' with URL '{}'", name, url);
    Ok(())
}

/// Remove a remote from the repository.
pub fn remove_remote(repo: &Repository, name: &str) -> Result<()> {
    repo.remote_delete(name)?;
    tracing::debug!("Removed remote '{}'", name);
    Ok(())
}

/// Rename a remote.
pub fn rename_remote(repo: &Repository, old_name: &str, new_name: &str) -> Result<()> {
    // git2 returns a string array of problems (refspecs that couldn't be renamed)
    let problems = repo.remote_rename(old_name, new_name)?;
    if !problems.is_empty() {
        tracing::warn!(
            "Some refspecs could not be renamed: {:?}",
            problems.iter().collect::<Vec<_>>()
        );
    }
    tracing::debug!("Renamed remote '{}' to '{}'", old_name, new_name);
    Ok(())
}

/// Set the URL of an existing remote.
pub fn set_remote_url(repo: &Repository, name: &str, url: &str) -> Result<()> {
    repo.remote_set_url(name, url)?;
    tracing::debug!("Set remote '{}' URL to '{}'", name, url);
    Ok(())
}

/// List all remotes in the repository.
pub fn list_remotes(repo: &Repository) -> Result<Vec<String>> {
    let remotes = repo.remotes()?;
    let names: Vec<String> = remotes.iter().filter_map(|n| n.map(String::from)).collect();
    Ok(names)
}

/// Check if a remote exists.
pub fn remote_exists(repo: &Repository, name: &str) -> bool {
    repo.find_remote(name).is_ok()
}

/// Repoint all local branches that track `old_remote` to track `new_remote` instead.
///
/// After `git remote rename origin upstream` + `git remote add origin <gate>`,
/// local branches still track `upstream`. This function updates them to track
/// the new `origin` so that `git pull` and `git push` go through the gate.
pub fn repoint_tracking_branches(
    repo_path: &std::path::Path,
    old_remote: &str,
    new_remote: &str,
) -> Result<()> {
    use super::cmd::{run_git, run_git_unchecked};

    tracing::debug!(
        "Repointing branches from '{}' to '{}' in {}",
        old_remote,
        new_remote,
        repo_path.display()
    );

    // List all branches that track old_remote
    let output = run_git(
        repo_path,
        &[
            "for-each-ref",
            "--format=%(refname:short) %(upstream:remotename)",
            "refs/heads/",
        ],
        "list branches",
    )?;

    let branch_info = String::from_utf8_lossy(&output.stdout);

    for line in branch_info.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.len() != 2 {
            continue;
        }

        let branch_name = parts[0];
        let tracking_remote = parts[1];

        if tracking_remote != old_remote {
            continue;
        }

        // Repoint this branch to track new_remote/<branch>
        let upstream_ref = format!("{}/{}", new_remote, branch_name);
        let set_upstream_output = run_git_unchecked(
            repo_path,
            &["branch", "--set-upstream-to", &upstream_ref, branch_name],
            "branch --set-upstream-to",
        )?;

        if set_upstream_output.status.success() {
            tracing::debug!(
                "Repointed branch '{}' from '{}' to '{}'",
                branch_name,
                old_remote,
                new_remote
            );
        } else {
            let stderr = String::from_utf8_lossy(&set_upstream_output.stderr);
            tracing::warn!(
                "Failed to repoint branch '{}': {}",
                branch_name,
                stderr.trim()
            );
        }
    }

    Ok(())
}
