//! Handler utility functions.
//!
//! Shared helpers used across multiple handlers.

use crate::ipc::{error_codes, ArtifactInfo, Response};
use airlock_core::AirlockPaths;
use serde::de::DeserializeOwned;
use std::time::{SystemTime, UNIX_EPOCH};

/// Parse JSON-RPC parameters into a typed struct, returning an error response on failure.
#[allow(clippy::result_large_err)]
pub fn parse_params<T: DeserializeOwned>(
    params: serde_json::Value,
    id: &serde_json::Value,
) -> Result<T, Response> {
    serde_json::from_value(params).map_err(|e| {
        Response::error(
            id.clone(),
            error_codes::INVALID_PARAMS,
            format!("Invalid parameters: {}", e),
        )
    })
}

/// Get current Unix timestamp.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Load artifacts from a run's artifact directory.
///
/// This function scans the run artifact directory and returns information about
/// all artifact files found, including:
/// - Top-level files (description.json, pr_result.json, etc.)
/// - Content artifacts in the content/ subdirectory (markdown files)
///
/// Log files in the logs/ subdirectory are not included.
///
/// Directory structure:
/// ```
/// ~/.airlock/artifacts/<repo-id>/<run-id>/
/// ├── logs/              # Stage log files (not included)
/// │   ├── describe/
/// │   │   ├── stdout.log
/// │   │   └── stderr.log
/// │   └── test/
/// │       └── ...
/// ├── content/           # Content artifacts (included)
/// │   └── <uuid>.md
/// ├── description.json   # From describe stage
/// ├── pr_result.json     # From create-pr stage
/// └── ...
/// ```
pub fn load_artifacts(paths: &AirlockPaths, repo_id: &str, run_id: &str) -> Vec<ArtifactInfo> {
    let artifact_dir = paths.run_artifacts(repo_id, run_id);
    let mut artifacts = Vec::new();

    // Check if artifact directory exists
    if !artifact_dir.exists() {
        return artifacts;
    }

    // Scan top-level entries
    if let Ok(entries) = std::fs::read_dir(&artifact_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            if path.is_dir() {
                // Scan artifact subdirectories per spec (Section 7.2):
                // - content/  → markdown content artifacts
                // - comments/ → code review comments (JSON)
                // - patches/  → suggested code changes (JSON)
                // Skip logs/ directory
                match name.as_str() {
                    "content" | "comments" | "patches" => {
                        artifacts.extend(load_subdirectory_artifacts(&path, &name));
                    }
                    _ => {} // Skip logs/ and any other directories
                }
                continue;
            }

            // Skip log files
            if name.ends_with(".log") {
                continue;
            }

            // Determine artifact type from filename
            let artifact_type = determine_artifact_type(&name);

            // Get file metadata
            let metadata = std::fs::metadata(&path);
            let size_bytes = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            let created_at = metadata
                .as_ref()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            artifacts.push(ArtifactInfo {
                name,
                path: path.to_string_lossy().to_string(),
                artifact_type,
                size_bytes,
                created_at,
            });
        }
    }

    // Sort by creation time for chronological ordering
    artifacts.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    artifacts
}

/// Load artifacts from a subdirectory (content/, comments/, patches/).
///
/// Per spec (Section 7.2), artifacts are organized by type:
/// - content/  → markdown files (airlock artifact content)
/// - comments/ → JSON files (airlock artifact comment)
/// - patches/  → JSON files (airlock artifact patch)
fn load_subdirectory_artifacts(dir: &std::path::Path, dir_name: &str) -> Vec<ArtifactInfo> {
    let mut artifacts = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            // Only include files (skip any subdirectories)
            if !path.is_file() {
                continue;
            }

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            // Get file metadata
            let metadata = std::fs::metadata(&path);
            let size_bytes = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            let created_at = metadata
                .as_ref()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            // Artifact type is "file" so UI can identify by path pattern
            // (e.g., /content/*.md, /comments/*.json, /patches/*.json)
            artifacts.push(ArtifactInfo {
                name,
                path: path.to_string_lossy().to_string(),
                artifact_type: "file".to_string(),
                size_bytes,
                created_at,
            });
        }
    }

    tracing::debug!("Loaded {} artifacts from {}/", artifacts.len(), dir_name);

    artifacts
}

/// Determine artifact type from filename.
fn determine_artifact_type(filename: &str) -> String {
    // Map known filenames to artifact types
    match filename {
        "description.json" | "description.md" => "description".to_string(),
        "test_result.json" => "test_results".to_string(),
        "push_result.json" => "push".to_string(),
        "pr_result.json" => "pr".to_string(),
        "tour.json" => "tour".to_string(),
        "diff_analysis.json" => "analysis".to_string(),
        _ => {
            // Fall back to extension as type
            std::path::Path::new(filename)
                .extension()
                .map(|ext| ext.to_string_lossy().to_lowercase())
                .unwrap_or_else(|| "unknown".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_artifacts_with_subdirectories() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());

        // Create artifact directory structure per spec (Section 7.2)
        let artifact_dir = paths.run_artifacts("repo-1", "run-1");
        std::fs::create_dir_all(&artifact_dir).unwrap();

        // Create top-level artifact
        std::fs::write(
            artifact_dir.join("description.json"),
            r#"{"title": "Test"}"#,
        )
        .unwrap();

        // Create content/ directory with markdown files
        let content_dir = artifact_dir.join("content");
        std::fs::create_dir_all(&content_dir).unwrap();
        std::fs::write(
            content_dir.join("summary.md"),
            "---\ntitle: \"Summary\"\n---\n\n# Summary",
        )
        .unwrap();

        // Create comments/ directory with JSON files
        let comments_dir = artifact_dir.join("comments");
        std::fs::create_dir_all(&comments_dir).unwrap();
        std::fs::write(
            comments_dir.join("ai-review.json"),
            r#"{"comments": [{"file": "src/main.rs", "line": 10, "message": "Test"}]}"#,
        )
        .unwrap();

        // Create patches/ directory with JSON files
        let patches_dir = artifact_dir.join("patches");
        std::fs::create_dir_all(&patches_dir).unwrap();
        std::fs::write(
            patches_dir.join("lint-fixes.json"),
            r#"{"title": "Lint fixes", "diff": "..."}"#,
        )
        .unwrap();

        // Create logs/ directory (should be skipped)
        let logs_dir = artifact_dir.join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();
        std::fs::write(logs_dir.join("test.log"), "log content").unwrap();

        // Load artifacts
        let artifacts = load_artifacts(&paths, "repo-1", "run-1");

        // Should have 4 artifacts: description.json + 1 content + 1 comment + 1 patch
        assert_eq!(artifacts.len(), 4);

        // Check that we have all expected artifacts
        let names: Vec<&str> = artifacts.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"description.json"));
        assert!(names.contains(&"summary.md"));
        assert!(names.contains(&"ai-review.json"));
        assert!(names.contains(&"lint-fixes.json"));

        // Check paths contain correct subdirectories
        let content_artifact = artifacts.iter().find(|a| a.name == "summary.md").unwrap();
        assert!(content_artifact.path.contains("/content/"));

        let comment_artifact = artifacts
            .iter()
            .find(|a| a.name == "ai-review.json")
            .unwrap();
        assert!(comment_artifact.path.contains("/comments/"));

        let patch_artifact = artifacts
            .iter()
            .find(|a| a.name == "lint-fixes.json")
            .unwrap();
        assert!(patch_artifact.path.contains("/patches/"));

        // All artifacts should have a non-zero created_at timestamp
        for artifact in &artifacts {
            assert!(
                artifact.created_at > 0,
                "artifact '{}' should have a non-zero created_at",
                artifact.name
            );
        }
    }
}
