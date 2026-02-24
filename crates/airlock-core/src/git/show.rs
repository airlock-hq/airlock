//! Git show utilities for reading file contents from specific commits.

use std::path::Path;
use std::process::Command;

use crate::error::{AirlockError, Result};

/// Read a file's contents from a specific commit in a git repository.
///
/// Runs `git show <commit>:<file_path>` against the given repo.
///
/// # Arguments
///
/// * `repo_path` - Path to the git repository (can be bare or working tree)
/// * `commit` - The commit SHA to read from
/// * `file_path` - The path of the file within the repository
///
/// # Returns
///
/// The file contents as a string, or an error if the file doesn't exist
/// at the given commit or the command fails.
pub fn show_file(repo_path: &Path, commit: &str, file_path: &str) -> Result<String> {
    let rev_spec = format!("{}:{}", commit, file_path);
    let output = Command::new("git")
        .args(["-C", &repo_path.to_string_lossy(), "show", &rev_spec])
        .output()
        .map_err(|e| AirlockError::Git(format!("Failed to run git show: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AirlockError::Git(format!(
            "git show {} failed: {}",
            rev_spec,
            stderr.trim()
        )));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| AirlockError::Git(format!("git show output is not valid UTF-8: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command as StdCommand;
    use tempfile::TempDir;

    /// Helper to run git commands in a directory.
    fn git(args: &[&str], cwd: &Path) -> String {
        let output = StdCommand::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("git command failed");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn test_show_file_reads_committed_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();

        git(&["init"], path);
        fs::write(path.join("hello.txt"), "hello world\n").unwrap();
        git(&["add", "hello.txt"], path);
        git(&["commit", "-m", "initial"], path);

        let sha = git(&["rev-parse", "HEAD"], path);
        let content = show_file(path, &sha, "hello.txt").unwrap();
        assert_eq!(content, "hello world\n");
    }

    #[test]
    fn test_show_file_returns_error_for_missing_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path();

        git(&["init"], path);
        fs::write(path.join("hello.txt"), "hello\n").unwrap();
        git(&["add", "hello.txt"], path);
        git(&["commit", "-m", "initial"], path);

        let sha = git(&["rev-parse", "HEAD"], path);
        let result = show_file(path, &sha, "nonexistent.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_show_file_works_with_bare_repo() {
        let dir = TempDir::new().unwrap();
        let work_path = dir.path().join("work");
        let bare_path = dir.path().join("bare.git");

        // Create a working repo with a commit
        fs::create_dir_all(&work_path).unwrap();
        git(&["init"], &work_path);
        fs::write(work_path.join("config.yaml"), "key: value\n").unwrap();
        git(&["add", "config.yaml"], &work_path);
        git(&["commit", "-m", "add config"], &work_path);
        let sha = git(&["rev-parse", "HEAD"], &work_path);

        // Create a bare repo and push to it
        git(&["init", "--bare", bare_path.to_str().unwrap()], dir.path());
        git(
            &["remote", "add", "bare", bare_path.to_str().unwrap()],
            &work_path,
        );
        git(&["push", "bare", "master"], &work_path);

        // Read from the bare repo
        let content = show_file(&bare_path, &sha, "config.yaml").unwrap();
        assert_eq!(content, "key: value\n");
    }
}
