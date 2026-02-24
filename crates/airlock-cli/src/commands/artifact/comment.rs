//! Comment artifact command.
//!
//! Produces code review comment artifacts for inline display.
//!
//! Usage:
//!   # Single comment
//!   airlock artifact comment --file src/auth.rs --line 42 --message "Token expiry not validated" --severity warning
//!
//!   # Batch mode (JSON array)
//!   airlock artifact comment --batch-file findings.json

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

/// Arguments for the comment command.
#[derive(Debug)]
pub struct CommentArgs {
    /// Single comment: file path
    pub file: Option<String>,
    /// Single comment: line number
    pub line: Option<u32>,
    /// Single comment: message
    pub message: Option<String>,
    /// Single comment: severity (info, warning, error)
    pub severity: Option<String>,
    /// Batch mode: JSON file with array of comments
    pub batch_file: Option<PathBuf>,
}

/// A single code comment.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CodeComment {
    /// File path relative to repo root.
    pub file: String,
    /// Line number (1-indexed).
    pub line: u32,
    /// Comment message.
    pub message: String,
    /// Severity: "info", "warning", "error".
    #[serde(default = "default_severity")]
    pub severity: String,
}

fn default_severity() -> String {
    "info".to_string()
}

/// Container for comments.
#[derive(Debug, Serialize, Deserialize)]
pub struct CommentsArtifact {
    pub comments: Vec<CodeComment>,
}

/// Execute the comment artifact command.
///
/// Creates comment artifacts in `$AIRLOCK_ARTIFACTS/comments/<id>.json`.
pub async fn comment(args: CommentArgs) -> Result<()> {
    // Get the artifacts directory from environment
    let artifacts_dir = std::env::var("AIRLOCK_ARTIFACTS")
        .context("AIRLOCK_ARTIFACTS environment variable not set. This command must be run within a pipeline stage.")?;
    let artifacts_path = PathBuf::from(&artifacts_dir);

    // Build comments from arguments
    let comments = if let Some(batch_file) = &args.batch_file {
        // Batch mode: read JSON file
        let content = std::fs::read_to_string(batch_file)
            .with_context(|| format!("Failed to read batch file: {:?}", batch_file))?;

        // Try to parse as array of comments directly, or as CommentsArtifact
        let parsed: Result<Vec<CodeComment>, _> = serde_json::from_str(&content);
        if let Ok(comments) = parsed {
            comments
        } else {
            let artifact: CommentsArtifact = serde_json::from_str(&content)
                .with_context(|| format!("Failed to parse batch file as JSON: {:?}", batch_file))?;
            artifact.comments
        }
    } else {
        // Single comment mode
        let file = args
            .file
            .ok_or_else(|| anyhow::anyhow!("--file is required for single comment mode"))?;
        let line = args
            .line
            .ok_or_else(|| anyhow::anyhow!("--line is required for single comment mode"))?;
        let message = args
            .message
            .ok_or_else(|| anyhow::anyhow!("--message is required for single comment mode"))?;
        let severity = args.severity.unwrap_or_else(|| "info".to_string());

        // Validate severity
        if !["info", "warning", "error"].contains(&severity.as_str()) {
            anyhow::bail!(
                "Invalid severity '{}'. Must be one of: info, warning, error",
                severity
            );
        }

        vec![CodeComment {
            file,
            line,
            message,
            severity,
        }]
    };

    if comments.is_empty() {
        info!("No comments to add");
        return Ok(());
    }

    // Create comments directory
    let comments_dir = artifacts_path.join("comments");
    std::fs::create_dir_all(&comments_dir)
        .with_context(|| format!("Failed to create comments directory: {:?}", comments_dir))?;

    // Generate unique ID
    let id = uuid::Uuid::new_v4().to_string();

    // Write JSON artifact
    let output_path = comments_dir.join(format!("{}.json", id));
    let artifact = CommentsArtifact { comments };
    let json_content =
        serde_json::to_string_pretty(&artifact).context("Failed to serialize comments artifact")?;

    std::fs::write(&output_path, &json_content)
        .with_context(|| format!("Failed to write comments artifact: {:?}", output_path))?;

    info!(
        "Created comments artifact with {} comments: {:?}",
        artifact.comments.len(),
        output_path
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    #[tokio::test]
    #[serial]
    async fn test_comment_single() {
        let temp_dir = TempDir::new().unwrap();
        let artifacts_dir = temp_dir.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        std::env::set_var("AIRLOCK_ARTIFACTS", artifacts_dir.to_str().unwrap());

        let args = CommentArgs {
            file: Some("src/main.rs".to_string()),
            line: Some(42),
            message: Some("Consider using a constant here".to_string()),
            severity: Some("warning".to_string()),
            batch_file: None,
        };

        comment(args).await.unwrap();

        // Verify comments directory was created
        let comments_dir = artifacts_dir.join("comments");
        assert!(comments_dir.exists());

        // Verify file was created
        let files: Vec<_> = std::fs::read_dir(&comments_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1);

        // Verify content
        let file_content = std::fs::read_to_string(files[0].path()).unwrap();
        let artifact: CommentsArtifact = serde_json::from_str(&file_content).unwrap();
        assert_eq!(artifact.comments.len(), 1);
        assert_eq!(artifact.comments[0].file, "src/main.rs");
        assert_eq!(artifact.comments[0].line, 42);
        assert_eq!(artifact.comments[0].severity, "warning");

        std::env::remove_var("AIRLOCK_ARTIFACTS");
    }

    #[tokio::test]
    #[serial]
    async fn test_comment_batch() {
        let temp_dir = TempDir::new().unwrap();
        let artifacts_dir = temp_dir.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        std::env::set_var("AIRLOCK_ARTIFACTS", artifacts_dir.to_str().unwrap());

        // Create batch file
        let batch_content = r#"[
            {"file": "src/a.rs", "line": 10, "message": "First comment", "severity": "info"},
            {"file": "src/b.rs", "line": 20, "message": "Second comment", "severity": "error"}
        ]"#;
        let batch_file = temp_dir.path().join("batch.json");
        std::fs::write(&batch_file, batch_content).unwrap();

        let args = CommentArgs {
            file: None,
            line: None,
            message: None,
            severity: None,
            batch_file: Some(batch_file),
        };

        comment(args).await.unwrap();

        // Verify file was created with both comments
        let comments_dir = artifacts_dir.join("comments");
        let files: Vec<_> = std::fs::read_dir(&comments_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1);

        let file_content = std::fs::read_to_string(files[0].path()).unwrap();
        let artifact: CommentsArtifact = serde_json::from_str(&file_content).unwrap();
        assert_eq!(artifact.comments.len(), 2);

        std::env::remove_var("AIRLOCK_ARTIFACTS");
    }

    #[tokio::test]
    #[serial]
    async fn test_comment_invalid_severity() {
        let temp_dir = TempDir::new().unwrap();
        let artifacts_dir = temp_dir.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        std::env::set_var("AIRLOCK_ARTIFACTS", artifacts_dir.to_str().unwrap());

        let args = CommentArgs {
            file: Some("src/main.rs".to_string()),
            line: Some(42),
            message: Some("Test".to_string()),
            severity: Some("critical".to_string()), // Invalid
            batch_file: None,
        };

        let result = comment(args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid severity"));

        std::env::remove_var("AIRLOCK_ARTIFACTS");
    }
}
