//! Repository operations for Airlock.

use crate::error::{AirlockError, Result};
use git2::Repository;
use std::fs;
use std::path::Path;

/// Create a new bare Git repository at the specified path.
///
/// The parent directory must exist, but the target path must not exist.
pub fn create_bare_repo(path: &Path) -> Result<Repository> {
    if path.exists() {
        return Err(AirlockError::Git(format!(
            "Path already exists: {}",
            path.display()
        )));
    }

    // Create the directory
    fs::create_dir_all(path)?;

    // Initialize as bare repository
    let repo = Repository::init_bare(path)?;

    tracing::debug!("Created bare repository at {}", path.display());
    Ok(repo)
}

/// Open an existing Git repository (bare or working).
pub fn open_repo(path: &Path) -> Result<Repository> {
    let repo = Repository::open(path)?;
    Ok(repo)
}

/// Discover the Git repository containing the given path.
/// Walks up the directory tree to find the repository root.
pub fn discover_repo(path: &Path) -> Result<Repository> {
    let repo = Repository::discover(path)?;
    Ok(repo)
}

/// Check if a path is inside a Git repository.
pub fn is_git_repo(path: &Path) -> bool {
    Repository::discover(path).is_ok()
}

/// Get the repository ID from its path (the directory name without .git suffix).
pub fn get_repo_id_from_path(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.trim_end_matches(".git").to_string())
}

/// Get the working directory of a non-bare repository.
pub fn get_workdir(repo: &Repository) -> Result<&Path> {
    repo.workdir().ok_or_else(|| {
        AirlockError::Git("Repository is bare and has no working directory".to_string())
    })
}

/// Read the `core.sshCommand` config from a repository's local git config.
///
/// Returns `None` if no custom SSH command is configured at the repo level.
fn get_local_ssh_command(repo_path: &Path) -> Option<String> {
    use std::process::Command;

    let output = Command::new("git")
        .args(["-C", repo_path.to_str().unwrap_or(".")])
        .args(["config", "--local", "core.sshCommand"])
        .output()
        .ok()?;

    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

/// Check if a URL uses SSH transport (git@host:path or ssh:// scheme).
fn is_ssh_url(url: &str) -> bool {
    url.contains('@') || url.starts_with("ssh://")
}

/// Check if the system `ssh` binary is OpenSSH (supports `-o IdentitiesOnly=yes`).
/// Returns false if `ssh` is not found or is a different implementation (e.g., PuTTY/Plink).
fn is_openssh_available() -> bool {
    use std::process::Command;

    let output = match Command::new("ssh").arg("-V").output() {
        Ok(o) => o,
        Err(_) => return false,
    };

    // `ssh -V` prints to stderr (OpenSSH convention)
    let version = String::from_utf8_lossy(&output.stderr);
    version.contains("OpenSSH")
}

/// Configure the gate repo's SSH credentials to match the working repo.
///
/// Handles two common multi-SSH-key configurations:
///
/// 1. **Per-repo `core.sshCommand`** (e.g., `ssh -i ~/.ssh/key_for_this_repo`):
///    Copies the working repo's `core.sshCommand` to the gate.
///
/// 2. **Hostname-based `~/.ssh/config`** (IdentityFile per Host):
///    Sets `core.sshCommand = "ssh -o IdentitiesOnly=yes"` on the gate to prevent
///    the SSH agent from offering keys for the wrong GitHub account. SSH will still
///    read `~/.ssh/config` and use the correct IdentityFile for the hostname.
pub fn configure_gate_ssh(
    working_repo_path: &Path,
    gate_path: &Path,
    upstream_url: &str,
) -> Result<()> {
    use std::process::Command;

    // Case 1: Working repo has an explicit core.sshCommand — propagate it
    if let Some(ssh_command) = get_local_ssh_command(working_repo_path) {
        tracing::info!("Propagating core.sshCommand to gate: {}", ssh_command);

        let output = Command::new("git")
            .args(["-C", gate_path.to_str().unwrap_or(".")])
            .args(["config", "core.sshCommand", &ssh_command])
            .output()
            .map_err(|e| {
                AirlockError::Git(format!("Failed to set core.sshCommand on gate: {e}"))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AirlockError::Git(format!(
                "Failed to set core.sshCommand on gate: {}",
                stderr.trim()
            )));
        }

        return Ok(());
    }

    // Case 2: SSH URL without explicit core.sshCommand — pin identity
    // When users have multiple SSH keys (e.g., for different GitHub accounts),
    // the SSH agent may offer the wrong key first. Setting IdentitiesOnly=yes
    // forces SSH to only use the IdentityFile from ~/.ssh/config for the hostname,
    // preventing authentication with the wrong account.
    //
    // Only do this when OpenSSH is the SSH client (not PuTTY/Plink on Windows).
    if is_ssh_url(upstream_url) && is_openssh_available() {
        tracing::info!(
            "Setting IdentitiesOnly=yes on gate for SSH URL: {}",
            upstream_url
        );

        let output = Command::new("git")
            .args(["-C", gate_path.to_str().unwrap_or(".")])
            .args(["config", "core.sshCommand", "ssh -o IdentitiesOnly=yes"])
            .output()
            .map_err(|e| {
                AirlockError::Git(format!("Failed to set core.sshCommand on gate: {e}"))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AirlockError::Git(format!(
                "Failed to set core.sshCommand on gate: {}",
                stderr.trim()
            )));
        }
    }

    Ok(())
}

/// Read a git config value from a repository.
///
/// Runs `git config --get <key>` in the given repo directory, returning
/// the value if set, or `None` otherwise.
pub fn get_git_config(repo_path: &Path, key: &str) -> Option<String> {
    use std::process::Command;

    let output = Command::new("git")
        .args(["-C", repo_path.to_str().unwrap_or(".")])
        .args(["config", "--get", key])
        .output()
        .ok()?;

    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

/// Get the current branch name (for non-bare repositories).
pub fn get_current_branch(repo: &Repository) -> Result<Option<String>> {
    let head = match repo.head() {
        Ok(head) => head,
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    if head.is_branch() {
        Ok(head.shorthand().map(String::from))
    } else {
        Ok(None)
    }
}
