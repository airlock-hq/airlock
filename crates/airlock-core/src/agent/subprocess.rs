//! Shared subprocess spawning and JSONL line-reading utilities.
//!
//! Non-SDK adapters (e.g., Codex) spawn their agent CLI as a subprocess,
//! read JSONL from stdout, and map each line to [`AgentEvent`] values.
//! This module provides the common plumbing so individual adapters only
//! need to supply the JSON→AgentEvent mapping logic.

use std::path::Path;
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tracing::debug;

use crate::error::{AirlockError, Result};

/// A running subprocess with a line reader over its stdout.
#[derive(Debug)]
pub struct SubprocessReader {
    /// The underlying child process.
    child: Child,
    /// Buffered line reader over the child's stdout.
    lines: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
}

impl SubprocessReader {
    /// Spawn a command and prepare a line reader over its stdout.
    ///
    /// The command's stdout is piped; stderr is inherited (goes to the
    /// parent process's stderr for real-time logging).
    pub fn spawn(program: &str, args: &[&str], cwd: Option<&Path>) -> Result<Self> {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .stdin(Stdio::null())
            .kill_on_drop(true);

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        debug!("Spawning subprocess: {} {:?}", program, args);

        let mut child = cmd
            .spawn()
            .map_err(|e| AirlockError::Agent(format!("Failed to spawn {}: {}", program, e)))?;

        let stdout = child.stdout.take().ok_or_else(|| {
            AirlockError::Agent(format!("Failed to capture stdout from {}", program))
        })?;

        let reader = BufReader::new(stdout);
        let lines = reader.lines();

        Ok(Self { child, lines })
    }

    /// Read the next line from stdout.
    ///
    /// Returns `None` when the subprocess has closed stdout (i.e., exited).
    pub async fn next_line(&mut self) -> Result<Option<String>> {
        match self.lines.next_line().await {
            Ok(line) => Ok(line),
            Err(e) => Err(AirlockError::Agent(format!(
                "Error reading subprocess stdout: {}",
                e
            ))),
        }
    }

    /// Wait for the subprocess to exit and return its exit status.
    pub async fn wait(&mut self) -> Result<std::process::ExitStatus> {
        self.child
            .wait()
            .await
            .map_err(|e| AirlockError::Agent(format!("Error waiting for subprocess: {}", e)))
    }
}

/// Parse a single line of JSONL into a [`serde_json::Value`].
///
/// Returns `None` for empty/whitespace-only lines (which are common between
/// JSONL events).
pub fn parse_jsonl_line(line: &str) -> Result<Option<serde_json::Value>> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let value: serde_json::Value = serde_json::from_str(trimmed).map_err(|e| {
        AirlockError::Agent(format!("Failed to parse JSONL: {} (line: {})", e, trimmed))
    })?;
    Ok(Some(value))
}

/// Check whether a CLI tool is available on PATH.
pub fn is_cli_available(name: &str) -> bool {
    let result = if cfg!(target_os = "windows") {
        std::process::Command::new("where").arg(name).output()
    } else {
        std::process::Command::new("which").arg(name).output()
    };

    match result {
        Ok(output) => output.status.success(),
        Err(e) => {
            debug!("Failed to check for {} CLI: {}", name, e);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_jsonl_line_valid() {
        let line = r#"{"type":"thread.started","thread_id":"abc"}"#;
        let result = parse_jsonl_line(line).unwrap();
        assert!(result.is_some());
        let val = result.unwrap();
        assert_eq!(val["type"], "thread.started");
        assert_eq!(val["thread_id"], "abc");
    }

    #[test]
    fn test_parse_jsonl_line_empty() {
        assert!(parse_jsonl_line("").unwrap().is_none());
        assert!(parse_jsonl_line("   ").unwrap().is_none());
        assert!(parse_jsonl_line("\n").unwrap().is_none());
    }

    #[test]
    fn test_parse_jsonl_line_invalid() {
        let result = parse_jsonl_line("not json");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to parse JSONL"));
    }

    #[test]
    fn test_parse_jsonl_line_with_whitespace() {
        let line = r#"  {"type":"turn.started"}  "#;
        let result = parse_jsonl_line(line).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap()["type"], "turn.started");
    }

    #[test]
    fn test_is_cli_available_nonexistent() {
        assert!(!is_cli_available("nonexistent-cli-tool-xyz-12345"));
    }

    #[test]
    fn test_is_cli_available_common_tool() {
        // `ls` or `dir` should be available on any system
        if cfg!(target_os = "windows") {
            // On Windows, `where` itself is available
            assert!(is_cli_available("where"));
        } else {
            assert!(is_cli_available("ls"));
        }
    }

    #[tokio::test]
    async fn test_subprocess_reader_echo() {
        // Use echo to test basic subprocess reading
        let reader = SubprocessReader::spawn("echo", &["hello world"], None);
        assert!(reader.is_ok());
        let mut reader = reader.unwrap();

        let line = reader.next_line().await.unwrap();
        assert_eq!(line, Some("hello world".to_string()));

        // Next read should return None (EOF)
        let line = reader.next_line().await.unwrap();
        assert!(line.is_none());

        let status = reader.wait().await.unwrap();
        assert!(status.success());
    }

    #[tokio::test]
    async fn test_subprocess_reader_multiline() {
        // printf outputs multiple lines
        let reader = SubprocessReader::spawn("printf", &["line1\\nline2\\nline3\\n"], None);
        assert!(reader.is_ok());
        let mut reader = reader.unwrap();

        let mut lines = Vec::new();
        while let Some(line) = reader.next_line().await.unwrap() {
            lines.push(line);
        }
        assert_eq!(lines, vec!["line1", "line2", "line3"]);
    }

    #[tokio::test]
    async fn test_subprocess_reader_spawn_failure() {
        let result = SubprocessReader::spawn("nonexistent-program-xyz", &[], None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to spawn"));
    }

    #[tokio::test]
    async fn test_subprocess_reader_with_cwd() {
        let reader = SubprocessReader::spawn("pwd", &[], Some(Path::new("/tmp")));
        assert!(reader.is_ok());
        let mut reader = reader.unwrap();

        let line = reader.next_line().await.unwrap();
        // /tmp may resolve to /private/tmp on macOS
        assert!(
            line.as_deref() == Some("/tmp") || line.as_deref() == Some("/private/tmp"),
            "unexpected cwd: {:?}",
            line
        );
    }
}
