//! Push operations for Airlock (CLI-based for proper credential support).

use super::cmd::run_git;
use super::refs::is_null_sha;
use crate::error::Result;
use crate::types::RefUpdate;
use std::path::Path;

/// Push refs to a remote using git CLI.
///
/// `refspecs` should be in the format "refs/heads/branch:refs/heads/branch"
/// or just "refs/heads/branch" for same-name push.
///
/// This uses the git CLI instead of libgit2 to properly support
/// SSH agents, credential helpers, and other authentication methods.
pub fn push(repo_path: &Path, remote_name: &str, refspecs: &[&str]) -> Result<()> {
    if refspecs.is_empty() {
        return Ok(());
    }

    tracing::debug!(
        "Pushing {:?} to '{}' in {}",
        refspecs,
        remote_name,
        repo_path.display()
    );

    let mut args: Vec<&str> = vec!["push", remote_name];
    args.extend_from_slice(refspecs);
    run_git(repo_path, &args, "push")?;

    tracing::debug!("Pushed to remote '{}'", remote_name);
    Ok(())
}

/// Push a single branch to a remote using git CLI.
pub fn push_branch(repo_path: &Path, remote_name: &str, branch_name: &str) -> Result<()> {
    let refspec = format!("refs/heads/{}:refs/heads/{}", branch_name, branch_name);
    push(repo_path, remote_name, &[&refspec])
}

/// Push all branches to a remote using git CLI.
pub fn push_all_branches(repo_path: &Path, remote_name: &str) -> Result<()> {
    tracing::debug!(
        "Pushing all branches to '{}' in {}",
        remote_name,
        repo_path.display()
    );

    run_git(repo_path, &["push", "--all", remote_name], "push")?;

    tracing::debug!("Pushed all branches to remote '{}'", remote_name);
    Ok(())
}

/// Build a refspec for a ref update.
///
/// - Deletions: `:refs/heads/branch` (empty source = delete)
/// - Creates/Updates: `ref:ref`
pub fn build_refspec(update: &RefUpdate) -> String {
    if is_null_sha(&update.new_sha) {
        format!(":{}", update.ref_name)
    } else {
        format!("{}:{}", update.ref_name, update.ref_name)
    }
}

/// Push multiple ref updates to a remote.
pub fn push_ref_updates(repo_path: &Path, remote_name: &str, updates: &[&RefUpdate]) -> Result<()> {
    if updates.is_empty() {
        return Ok(());
    }
    let refspecs: Vec<String> = updates.iter().map(|u| build_refspec(u)).collect();
    let refspec_refs: Vec<&str> = refspecs.iter().map(|s| s.as_str()).collect();
    push(repo_path, remote_name, &refspec_refs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_refspec_for_create() {
        let update = RefUpdate {
            ref_name: "refs/heads/feature".to_string(),
            old_sha: "0000000000000000000000000000000000000000".to_string(),
            new_sha: "abc123def456".to_string(),
        };
        assert_eq!(
            build_refspec(&update),
            "refs/heads/feature:refs/heads/feature"
        );
    }

    #[test]
    fn test_build_refspec_for_update() {
        let update = RefUpdate {
            ref_name: "refs/heads/main".to_string(),
            old_sha: "abc123".to_string(),
            new_sha: "def456".to_string(),
        };
        assert_eq!(build_refspec(&update), "refs/heads/main:refs/heads/main");
    }

    #[test]
    fn test_build_refspec_for_deletion() {
        let update = RefUpdate {
            ref_name: "refs/heads/old-branch".to_string(),
            old_sha: "abc123def456".to_string(),
            new_sha: "0000000000000000000000000000000000000000".to_string(),
        };
        assert_eq!(build_refspec(&update), ":refs/heads/old-branch");
    }

    #[test]
    fn test_build_refspec_for_tag() {
        let update = RefUpdate {
            ref_name: "refs/tags/v1.0.0".to_string(),
            old_sha: "0000000000000000000000000000000000000000".to_string(),
            new_sha: "abc123def456".to_string(),
        };
        assert_eq!(build_refspec(&update), "refs/tags/v1.0.0:refs/tags/v1.0.0");
    }

    #[test]
    fn test_build_refspec_for_tag_deletion() {
        let update = RefUpdate {
            ref_name: "refs/tags/v1.0.0".to_string(),
            old_sha: "abc123def456".to_string(),
            new_sha: "0000000000000000000000000000000000000000".to_string(),
        };
        assert_eq!(build_refspec(&update), ":refs/tags/v1.0.0");
    }
}
