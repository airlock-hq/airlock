//! Content artifact command.
//!
//! Produces markdown content artifacts for display in the Push Request.
//!
//! Usage:
//!   airlock artifact content --title "Summary" < content.md
//!   airlock artifact content --title "Summary" --file summary.md

use anyhow::{Context, Result};
use std::io::{self, Read};
use std::path::PathBuf;
use tracing::info;

/// Arguments for the content command.
#[derive(Debug)]
pub struct ContentArgs {
    /// Title for the content section.
    pub title: String,
    /// File to read content from (if not using stdin).
    pub file: Option<PathBuf>,
}

/// Execute the content artifact command.
///
/// Reads content from stdin or a file and writes it as a content artifact
/// to `$AIRLOCK_ARTIFACTS/content/<id>.md` with YAML frontmatter.
pub async fn content(args: ContentArgs) -> Result<()> {
    // Get the artifacts directory from environment
    let artifacts_dir = std::env::var("AIRLOCK_ARTIFACTS")
        .context("AIRLOCK_ARTIFACTS environment variable not set. This command must be run within a pipeline stage.")?;
    let artifacts_path = PathBuf::from(&artifacts_dir);

    // Read content from file or stdin
    let content_text = if let Some(file_path) = &args.file {
        std::fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read content from file: {:?}", file_path))?
    } else {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .context("Failed to read content from stdin")?;
        buffer
    };

    if content_text.trim().is_empty() {
        anyhow::bail!("Content is empty. Provide content via stdin or --file.");
    }

    // Create content directory
    let content_dir = artifacts_path.join("content");
    std::fs::create_dir_all(&content_dir)
        .with_context(|| format!("Failed to create content directory: {:?}", content_dir))?;

    // Generate unique ID for this content
    let id = uuid::Uuid::new_v4().to_string();

    // Write markdown with YAML frontmatter
    let output_path = content_dir.join(format!("{}.md", id));
    let output_content = format!(
        "---\ntitle: \"{}\"\n---\n\n{}",
        args.title.replace('"', "\\\""),
        content_text
    );

    std::fs::write(&output_path, &output_content)
        .with_context(|| format!("Failed to write content artifact: {:?}", output_path))?;

    info!(
        "Created content artifact '{}': {:?}",
        args.title, output_path
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
    async fn test_content_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let artifacts_dir = temp_dir.path().join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();

        // Set environment variable
        std::env::set_var("AIRLOCK_ARTIFACTS", artifacts_dir.to_str().unwrap());

        // Create input file
        let input_file = temp_dir.path().join("input.md");
        std::fs::write(&input_file, "# Test Content\n\nThis is test content.").unwrap();

        let args = ContentArgs {
            title: "Test Title".to_string(),
            file: Some(input_file),
        };

        content(args).await.unwrap();

        // Verify content directory was created
        let content_dir = artifacts_dir.join("content");
        assert!(content_dir.exists());

        // Verify file was created
        let files: Vec<_> = std::fs::read_dir(&content_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1);

        // Verify content
        let file_content = std::fs::read_to_string(files[0].path()).unwrap();
        assert!(file_content.contains("title: \"Test Title\""));
        assert!(file_content.contains("# Test Content"));

        // Cleanup
        std::env::remove_var("AIRLOCK_ARTIFACTS");
    }

    #[tokio::test]
    #[serial]
    async fn test_content_requires_artifacts_env() {
        std::env::remove_var("AIRLOCK_ARTIFACTS");

        let args = ContentArgs {
            title: "Test".to_string(),
            file: None,
        };

        let result = content(args).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("AIRLOCK_ARTIFACTS"));
    }
}
