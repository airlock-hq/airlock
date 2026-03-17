//! Diff and tour IPC types shared between daemon and app.

use serde::{Deserialize, Serialize};

/// Information about a single commit with its diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitDiffInfo {
    pub sha: String,
    /// Subject line.
    pub message: String,
    pub author: String,
    /// Unix epoch seconds.
    pub timestamp: i64,
    /// Unified diff patch for this commit.
    pub patch: String,
    pub files_changed: Vec<String>,
    pub additions: u32,
    pub deletions: u32,
}

/// Result for the `get_run_diff` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetRunDiffResult {
    pub run_id: String,
    pub branch: String,
    pub base_sha: String,
    pub head_sha: String,

    /// Full unified diff patch string.
    pub patch: String,

    pub files_changed: Vec<String>,
    pub additions: u32,
    pub deletions: u32,

    /// Per-commit diff information (empty for single-commit pushes).
    #[serde(default)]
    pub commits: Vec<CommitDiffInfo>,
}

/// Diff hunk information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunkInfo {
    pub id: String,
    pub file_path: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub additions: u32,
    pub deletions: u32,
    pub content: String,
    pub language: Option<String>,
}

/// Intent diff result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentDiffResult {
    pub intent_id: String,
    pub hunks: Vec<DiffHunkInfo>,
}

/// Intent tour result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentTourResult {
    pub intent_id: String,
    pub tour: Option<TourInfo>,
}

/// Guided tour information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TourInfo {
    pub title: String,
    pub overview: String,
    pub steps: Vec<TourStepInfo>,
    pub estimated_minutes: u32,
}

/// A single step in a guided tour.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TourStepInfo {
    pub step_number: u32,
    pub title: String,
    pub explanation: String,
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub code_snippet: String,
    #[serde(default)]
    pub annotations: Vec<LineAnnotationInfo>,
    pub deep_dive: Option<String>,
}

/// An annotation attached to a specific line in a tour step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineAnnotationInfo {
    pub line_offset: u32,
    pub text: String,
    pub annotation_type: String,
}

/// Result of approving an intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApproveIntentResult {
    pub intent_id: String,
    pub success: bool,
    pub new_status: String,
}

/// Result of rejecting an intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectIntentResult {
    pub intent_id: String,
    pub success: bool,
    pub new_status: String,
}
