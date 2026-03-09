//! Core data types for Airlock.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

// =============================================================================
// Approval Mode
// =============================================================================

/// Controls when a step pauses for user approval.
///
/// In YAML, this is specified as:
/// - `false` or omitted → `Never` (default)
/// - `true` → `Always`
/// - `"if_patches"` → `IfPatches` (pause only when unapplied patches exist)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApprovalMode {
    /// Never pause for approval (default).
    #[default]
    Never,
    /// Always pause for approval after the step completes.
    Always,
    /// Pause only when there are pending (unapplied) patches.
    IfPatches,
}

impl Serialize for ApprovalMode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ApprovalMode::Never => serializer.serialize_bool(false),
            ApprovalMode::Always => serializer.serialize_bool(true),
            ApprovalMode::IfPatches => serializer.serialize_str("if_patches"),
        }
    }
}

impl<'de> Deserialize<'de> for ApprovalMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ApprovalModeVisitor;

        impl<'de> serde::de::Visitor<'de> for ApprovalModeVisitor {
            type Value = ApprovalMode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a boolean or the string \"if_patches\"")
            }

            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(if v {
                    ApprovalMode::Always
                } else {
                    ApprovalMode::Never
                })
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                match v {
                    "if_patches" => Ok(ApprovalMode::IfPatches),
                    "true" => Ok(ApprovalMode::Always),
                    "false" => Ok(ApprovalMode::Never),
                    other => Err(E::unknown_variant(other, &["true", "false", "if_patches"])),
                }
            }
        }

        deserializer.deserialize_any(ApprovalModeVisitor)
    }
}

// =============================================================================
// Step-Based Pipeline Types
// =============================================================================

/// Status of a pipeline step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// Step has not started yet.
    #[default]
    Pending,

    /// Step is currently executing.
    Running,

    /// Step completed successfully.
    Passed,

    /// Step failed (or was rejected during approval).
    Failed,

    /// Step was skipped.
    Skipped,

    /// Step has `require-approval=true` and awaits user approval.
    AwaitingApproval,
}

impl StepStatus {
    /// Returns true if this is a final status (no more transitions expected).
    pub fn is_final(&self) -> bool {
        matches!(self, Self::Passed | Self::Failed | Self::Skipped)
    }
}

/// Status of a job in the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// Job has not started yet.
    #[default]
    Pending,

    /// Job is currently executing.
    Running,

    /// Job completed successfully.
    Passed,

    /// Job failed.
    Failed,

    /// Job was skipped (e.g., due to a failed dependency).
    Skipped,

    /// Job has a step awaiting user approval.
    AwaitingApproval,
}

impl JobStatus {
    /// Returns true if this is a final status (no more transitions expected).
    pub fn is_final(&self) -> bool {
        matches!(self, Self::Passed | Self::Failed | Self::Skipped)
    }
}

/// Definition of a pipeline step (within a job).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDefinition {
    /// Step display name.
    pub name: String,

    /// Shell command to execute. Optional when `uses` is provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,

    /// Reference to a reusable action (e.g., "owner/repo/path@version").
    /// When provided, the action is fetched from GitHub and its `run` command is used.
    /// Inline properties (shell, continue-on-error, etc.) override the reusable action's defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uses: Option<String>,

    /// Shell to use (sh, bash, zsh). When omitted, uses the user's login shell
    /// (`$SHELL -l -c`) for full environment (API keys, PATH, version managers, etc.).
    /// When explicitly set, uses the specified shell with `-c`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,

    /// Environment variables for this step.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,

    /// Continue pipeline if this step fails. Defaults to false.
    #[serde(default, rename = "continue-on-error")]
    pub continue_on_error: bool,

    /// Pause for user approval after this step completes.
    /// Accepts `false` (never), `true` (always), or `"if_patches"` (only when patches pending).
    #[serde(default, rename = "require-approval")]
    pub require_approval: ApprovalMode,

    /// Maximum execution time for this step in seconds.
    /// When omitted, the executor applies a default timeout (60 minutes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,

    /// Auto-apply pending patches after this step passes.
    /// When `true`, the executor applies all patches from `$AIRLOCK_ARTIFACTS/patches/`
    /// and commits them, similar to running `airlock exec freeze`.
    #[serde(default, rename = "apply-patch")]
    pub apply_patch: bool,
}

impl StepDefinition {
    /// Returns the effective run command for this step.
    /// Returns None if neither `run` nor `uses` is set.
    pub fn effective_run(&self) -> Option<&str> {
        self.run.as_deref()
    }

    /// Returns true if this step uses a reusable action reference.
    pub fn is_reusable(&self) -> bool {
        self.uses.is_some()
    }
}

/// Result of a single step execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    /// Unique identifier for this step result.
    pub id: String,

    /// ID of the run this step belongs to.
    pub run_id: String,

    /// ID of the job this step belongs to.
    #[serde(default)]
    pub job_id: String,

    /// Step name (matches StepDefinition.name).
    pub name: String,

    /// Current status of the step.
    pub status: StepStatus,

    /// Order of this step within the job (0-indexed).
    /// Used to display steps in the correct order defined by the job config.
    #[serde(default)]
    pub step_order: i32,

    /// Exit code of the step command (if completed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,

    /// Duration of the step execution in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,

    /// Error message if the step failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Timestamp when the step started (Unix epoch seconds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,

    /// Timestamp when the step completed (Unix epoch seconds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
}

/// Result of a job execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResult {
    /// Unique identifier for this job result.
    pub id: String,

    /// ID of the run this job belongs to.
    pub run_id: String,

    /// Key from the jobs map (e.g., "lint", "verify").
    pub job_key: String,

    /// Display name for the job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Current status of the job.
    pub status: JobStatus,

    /// Topological order for display.
    #[serde(default)]
    pub job_order: i32,

    /// Timestamp when the job started (Unix epoch seconds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,

    /// Timestamp when the job completed (Unix epoch seconds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,

    /// Error message if the job failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Path to the worktree used by this job (for pool recovery on restart).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
}

/// A repository enrolled in Airlock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    /// Unique identifier for the repo (hash of origin URL + working path).
    pub id: String,

    /// Path to the working repository.
    pub working_path: PathBuf,

    /// Original upstream URL (e.g., git@github.com:user/repo.git).
    pub upstream_url: String,

    /// Path to the local bare repo gate.
    pub gate_path: PathBuf,

    /// Timestamp of last sync with upstream (Unix epoch seconds).
    pub last_sync: Option<i64>,

    /// Timestamp when the repo was enrolled (Unix epoch seconds).
    pub created_at: i64,
}

/// A ref update received from a push.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefUpdate {
    /// The ref name (e.g., refs/heads/main).
    pub ref_name: String,

    /// The old commit SHA (all zeros for new refs).
    pub old_sha: String,

    /// The new commit SHA.
    pub new_sha: String,
}

/// A pipeline run triggered by a push.
///
/// NOTE: This struct contains both legacy and new fields for backward compatibility
/// during the architecture refactor. Legacy fields are marked as DEPRECATED and will
/// be removed in steps 10.13-10.16.
///
/// In the new step-based model, run status is derived from step results, not stored directly.
/// Use the helper methods `is_running()`, `is_completed()`, etc. to check status
/// (requires passing step results).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    /// Unique identifier for the run.
    pub id: String,

    /// ID of the repository this run belongs to.
    pub repo_id: String,

    /// Ref updates that triggered this run.
    /// DEPRECATED: Use branch, base_sha, head_sha instead.
    #[serde(default)]
    pub ref_updates: Vec<RefUpdate>,

    // =========================================================================
    // Step-based pipeline fields
    // =========================================================================
    /// Branch being pushed (e.g., "refs/heads/feature/add-auth").
    #[serde(default)]
    pub branch: String,

    /// Base commit SHA (before push).
    #[serde(default)]
    pub base_sha: String,

    /// Head commit SHA (after push).
    #[serde(default)]
    pub head_sha: String,

    /// Name of the currently executing step (for display purposes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step: Option<String>,

    /// Error message if the run failed (pipeline-level errors).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Whether this run was superseded by a newer push to the same branch.
    #[serde(default)]
    pub superseded: bool,

    /// Workflow file that triggered this run (e.g., "main.yml").
    #[serde(default)]
    pub workflow_file: String,

    /// Display name from the workflow's `name:` field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_name: Option<String>,

    /// Timestamp when the run started (Unix epoch seconds).
    pub created_at: i64,

    /// Timestamp when the run was last updated (Unix epoch seconds).
    #[serde(default)]
    pub updated_at: i64,
}

impl Run {
    /// Returns true if this run was superseded by a newer push to the same branch.
    pub fn is_superseded(&self) -> bool {
        self.superseded
    }

    /// Returns true if the pipeline is still running.
    ///
    /// A run is considered running if any step is `Pending`, `Running`, or `AwaitingApproval`.
    pub fn is_running(&self, steps: &[StepResult]) -> bool {
        steps.iter().any(|s| {
            matches!(
                s.status,
                StepStatus::Pending | StepStatus::Running | StepStatus::AwaitingApproval
            )
        })
    }

    /// Returns true if the pipeline has completed (all steps have final status).
    ///
    /// A run is completed when all steps are `Passed`, `Failed`, or `Skipped`.
    pub fn is_completed(&self, steps: &[StepResult]) -> bool {
        !steps.is_empty() && steps.iter().all(|s| s.status.is_final())
    }

    /// Returns true if the pipeline failed (any step is `Failed`).
    pub fn is_failed(&self, steps: &[StepResult]) -> bool {
        steps.iter().any(|s| s.status == StepStatus::Failed)
    }

    /// Returns true if the pipeline completed successfully.
    ///
    /// All steps must be `Passed` or `Skipped`.
    pub fn is_successful(&self, steps: &[StepResult]) -> bool {
        !steps.is_empty()
            && steps
                .iter()
                .all(|s| matches!(s.status, StepStatus::Passed | StepStatus::Skipped))
    }

    /// Returns true if any step is awaiting user approval.
    pub fn is_awaiting_approval(&self, steps: &[StepResult]) -> bool {
        steps
            .iter()
            .any(|s| s.status == StepStatus::AwaitingApproval)
    }

    /// Get the derived status string for display purposes (from step results).
    ///
    /// DEPRECATED: Use `derived_status_from_jobs` instead for new code.
    /// Returns one of: "superseded", "running", "completed", "failed", "awaiting_approval", "pending"
    pub fn derived_status(&self, steps: &[StepResult]) -> &'static str {
        if self.is_superseded() {
            return "superseded";
        }
        if steps.is_empty() {
            return "pending";
        }
        if self.is_awaiting_approval(steps) {
            "awaiting_approval"
        } else if self.is_failed(steps) {
            "failed"
        } else if self.is_successful(steps) {
            "completed"
        } else if self.is_running(steps) {
            "running"
        } else {
            "pending"
        }
    }

    // =========================================================================
    // Job-based status derivation (new — derives from JobResult statuses)
    // =========================================================================

    /// Returns true if the run is still active based on job statuses.
    ///
    /// A run is running if any job is `Pending`, `Running`, or `AwaitingApproval`.
    pub fn is_running_from_jobs(&self, jobs: &[JobResult]) -> bool {
        jobs.iter().any(|j| {
            matches!(
                j.status,
                JobStatus::Pending | JobStatus::Running | JobStatus::AwaitingApproval
            )
        })
    }

    /// Returns true if all jobs have reached a final status.
    pub fn is_completed_from_jobs(&self, jobs: &[JobResult]) -> bool {
        !jobs.is_empty() && jobs.iter().all(|j| j.status.is_final())
    }

    /// Returns true if any job has failed.
    pub fn is_failed_from_jobs(&self, jobs: &[JobResult]) -> bool {
        jobs.iter().any(|j| j.status == JobStatus::Failed)
    }

    /// Returns true if all jobs passed or were skipped.
    pub fn is_successful_from_jobs(&self, jobs: &[JobResult]) -> bool {
        !jobs.is_empty()
            && jobs
                .iter()
                .all(|j| matches!(j.status, JobStatus::Passed | JobStatus::Skipped))
    }

    /// Returns true if any job is awaiting approval.
    pub fn is_awaiting_approval_from_jobs(&self, jobs: &[JobResult]) -> bool {
        jobs.iter().any(|j| j.status == JobStatus::AwaitingApproval)
    }

    /// Get the derived status string from job results.
    ///
    /// A run is:
    /// - `Running` if any job is `Running`
    /// - `AwaitingApproval` if any job is `AwaitingApproval` and none are `Running`
    /// - `Failed` if any job is `Failed` (and none running/awaiting)
    /// - `Passed` (returned as "completed") if all jobs are `Passed`
    ///
    /// Returns one of: "superseded", "running", "completed", "failed", "awaiting_approval", "pending"
    pub fn derived_status_from_jobs(&self, jobs: &[JobResult]) -> &'static str {
        if self.is_superseded() {
            return "superseded";
        }
        if self.error.is_some() {
            // Run has an error (e.g. "Stopped by user") — treat as failed
            // even if some jobs haven't transitioned to terminal states yet.
            return "failed";
        }
        if jobs.is_empty() {
            return "pending";
        }
        // Running takes highest priority (active execution)
        if jobs.iter().any(|j| j.status == JobStatus::Running) {
            return "running";
        }
        // Awaiting approval (no jobs running, but at least one awaiting)
        if self.is_awaiting_approval_from_jobs(jobs) {
            return "awaiting_approval";
        }
        // Failed (no running/awaiting, but at least one failed)
        if self.is_failed_from_jobs(jobs) {
            return "failed";
        }
        // All passed/skipped
        if self.is_successful_from_jobs(jobs) {
            return "completed";
        }
        // Some pending jobs remain
        if self.is_running_from_jobs(jobs) {
            return "running";
        }
        "pending"
    }
}

// =============================================================================
// Legacy Intent Types (To be removed in Steps 10.13-10.16)
// =============================================================================

/// Category of an intent.
///
/// DEPRECATED: Will be removed when intent-centric pipeline is removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IntentCategory {
    /// New functionality.
    Feature,

    /// Bug fix.
    Fix,

    /// Code restructuring without behavior change.
    Refactor,

    /// Documentation updates.
    Docs,

    /// Schema or database changes.
    Schema,

    /// Build, CI, or dependency updates.
    Chore,

    /// Test additions or updates.
    Test,

    /// Not yet categorized (to be determined by LLM analysis).
    #[default]
    Unknown,
}

/// Status of an individual intent in the pipeline.
///
/// DEPRECATED: Will be removed when intent-centric pipeline is removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IntentStatus {
    /// Intent has been created but not yet processed.
    #[default]
    Pending,

    /// Intent is currently being processed (analyze, clean, test, etc.).
    Processing,

    /// Intent processing is complete and ready for user review.
    ReadyForReview,

    /// User has approved the intent for forwarding.
    Approved,

    /// User has rejected the intent.
    Rejected,

    /// Intent has been forwarded to upstream (PR created).
    Forwarded,

    /// Intent processing failed with an error.
    Failed,
}

/// A logical unit of change within a run.
///
/// DEPRECATED: Will be removed when intent-centric pipeline is removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    /// Unique identifier for the intent.
    pub id: String,

    /// ID of the run this intent belongs to.
    pub run_id: String,

    /// Human-readable title for the intent.
    pub title: String,

    /// Category of the change.
    pub category: IntentCategory,

    /// Files changed in this intent.
    pub files: Vec<String>,

    /// Generated PR description (markdown).
    pub description: Option<String>,

    /// IDs of intents this one depends on.
    pub depends_on: Vec<String>,

    /// Order in which this intent should be applied.
    pub order: i32,

    /// Current status of the intent in the pipeline.
    #[serde(default)]
    pub status: IntentStatus,

    /// IDs of hunks assigned to this intent.
    #[serde(default)]
    pub hunk_ids: Vec<String>,

    /// Name of the branch created for this intent (e.g., "airlock/{run_id}/{intent_id}").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_name: Option<String>,

    /// URL of the PR created for this intent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,

    /// PR number for this intent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u32>,

    /// Error message if the intent failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Type alias for hunk dependency mapping.
/// Maps hunk_id to list of dependency hunk_ids.
///
/// DEPRECATED: Will be removed when intent-centric pipeline is removed.
pub type HunkDependencies = std::collections::HashMap<String, Vec<String>>;

/// Dependency graph for hunks.
///
/// DEPRECATED: Will be removed when intent-centric pipeline is removed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DependencyGraph {
    /// Raw dependency mapping: hunk_id -> list of hunk_ids it depends on.
    pub dependencies: HunkDependencies,

    /// Connected components (islands) of hunks.
    pub components: Vec<Vec<String>>,
}

/// A hunk with full context for intent splitting.
///
/// DEPRECATED: Will be removed when intent-centric pipeline is removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitHunk {
    /// Unique identifier for this hunk (format: "file_path:hunk_index").
    pub id: String,
    /// Path to the file this hunk belongs to.
    pub file_path: String,
    /// Index of this hunk within the file (0-based).
    pub hunk_index: u32,
    /// Starting line number in the old version.
    pub old_start: u32,
    /// Number of lines in the old version.
    pub old_lines: u32,
    /// Starting line number in the new version.
    pub new_start: u32,
    /// Number of lines in the new version.
    pub new_lines: u32,
    /// Lines added in this hunk.
    pub additions: u32,
    /// Lines deleted in this hunk.
    pub deletions: u32,
    /// The actual diff content of this hunk (unified diff format).
    pub content: String,
    /// Detected programming language for the file.
    pub language: Option<String>,
}

/// Result of intent splitting analysis.
///
/// DEPRECATED: Will be removed when intent-centric pipeline is removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitAnalysis {
    /// Run ID this analysis belongs to.
    pub run_id: String,
    /// The generated intents with their assigned hunks.
    pub intents: Vec<SplitIntent>,
    /// Total number of hunks analyzed.
    pub total_hunks: u32,
    /// Timestamp when analysis was performed.
    pub analyzed_at: i64,
}

/// An intent generated from the split analysis.
///
/// DEPRECATED: Will be removed when intent-centric pipeline is removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitIntent {
    /// Unique identifier for this intent.
    pub id: String,
    /// Human-readable title for the intent.
    pub title: String,
    /// Category of the change.
    pub category: IntentCategory,
    /// Description of the intent's purpose.
    pub description: Option<String>,
    /// IDs of hunks assigned to this intent.
    pub hunk_ids: Vec<String>,
    /// File paths affected by this intent.
    pub files: Vec<String>,
    /// IDs of other intents this one depends on.
    pub depends_on: Vec<String>,
    /// Order in which this intent should be applied.
    pub order: i32,
}

/// Analysis of a single ref update.
///
/// DEPRECATED: Will be removed when intent-centric pipeline is removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefAnalysis {
    /// The ref name (e.g., refs/heads/main).
    pub ref_name: String,
    /// Old commit SHA.
    pub old_sha: String,
    /// New commit SHA.
    pub new_sha: String,
    /// Whether this is a new branch/ref.
    pub is_new_ref: bool,
    /// Whether this is a deletion.
    pub is_deletion: bool,
    /// Files changed in this ref update.
    pub files: Vec<FileChange>,
    /// Total lines added across all files.
    pub total_additions: u32,
    /// Total lines deleted across all files.
    pub total_deletions: u32,
    /// Detected languages in this ref update.
    pub languages: Vec<String>,
    /// Suggested category for this change.
    pub suggested_category: IntentCategory,
}

/// Complete diff analysis for a run.
///
/// DEPRECATED: Will be removed when intent-centric pipeline is removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffAnalysis {
    /// ID of the run this analysis belongs to.
    pub run_id: String,
    /// Analysis for each ref update.
    pub refs: Vec<RefAnalysis>,
    /// All unique languages detected across all files.
    pub languages: Vec<String>,
    /// All unique files changed.
    pub files: Vec<String>,
    /// Total lines added across all refs.
    pub total_additions: u32,
    /// Total lines deleted across all refs.
    pub total_deletions: u32,
    /// Timestamp when analysis was performed.
    pub analyzed_at: i64,
}

/// A guided tour for an intent - step-by-step walkthrough of code changes.
///
/// DEPRECATED: Will be removed when intent-centric pipeline is removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidedTour {
    /// ID of the intent this tour belongs to.
    pub intent_id: String,
    /// Title of the tour (usually matches intent title).
    pub title: String,
    /// Brief overview of what the tour covers.
    pub overview: String,
    /// Ordered list of tour steps.
    pub steps: Vec<TourStep>,
    /// Total estimated reading time in minutes.
    pub estimated_minutes: u32,
    /// Timestamp when the tour was generated.
    pub generated_at: i64,
}

/// A single step in a guided tour.
///
/// DEPRECATED: Will be removed when intent-centric pipeline is removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TourStep {
    /// Step number (1-based).
    pub step_number: u32,
    /// Title for this step.
    pub title: String,
    /// Explanation of what's happening in this step.
    pub explanation: String,
    /// File path being shown in this step.
    pub file: String,
    /// Starting line number (1-based).
    pub start_line: u32,
    /// Ending line number (1-based).
    pub end_line: u32,
    /// The code snippet content being highlighted.
    pub code_snippet: String,
    /// Annotations for specific lines within this step.
    #[serde(default)]
    pub annotations: Vec<LineAnnotation>,
    /// Optional "deep dive" for complex sections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deep_dive: Option<String>,
}

/// An annotation attached to a specific line in a tour step.
///
/// DEPRECATED: Will be removed when intent-centric pipeline is removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineAnnotation {
    /// Line number within the snippet (relative to start_line).
    pub line_offset: u32,
    /// The annotation text.
    pub text: String,
    /// Type of annotation: "info", "warning", "important".
    #[serde(default = "default_annotation_type")]
    pub annotation_type: String,
}

fn default_annotation_type() -> String {
    "info".to_string()
}

/// Result of the tour generation stage for all intents.
///
/// DEPRECATED: Will be removed when intent-centric pipeline is removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TourResult {
    /// Run ID this result belongs to.
    pub run_id: String,
    /// Generated tours for each intent.
    pub tours: Vec<GuidedTour>,
    /// Timestamp when tours were generated.
    pub generated_at: i64,
}

// =============================================================================
// Sync Types
// =============================================================================

/// A record of an upstream sync operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncLog {
    /// Unique identifier for the sync.
    pub id: String,

    /// ID of the repository that was synced.
    pub repo_id: String,

    /// Whether the sync succeeded.
    pub success: bool,

    /// Error message if the sync failed.
    pub error: Option<String>,

    /// Timestamp of the sync (Unix epoch seconds).
    pub synced_at: i64,
}

// =============================================================================
// Diff Analysis Types
// =============================================================================

/// Status of a file in the diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    /// File was added.
    Added,
    /// File was deleted.
    Deleted,
    /// File was modified.
    Modified,
    /// File was renamed.
    Renamed,
    /// File was copied.
    Copied,
}

/// A hunk in a diff showing a contiguous block of changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    /// Starting line number in the old version.
    pub old_start: u32,
    /// Number of lines in the old version.
    pub old_lines: u32,
    /// Starting line number in the new version.
    pub new_start: u32,
    /// Number of lines in the new version.
    pub new_lines: u32,
    /// Lines added in this hunk.
    pub additions: u32,
    /// Lines deleted in this hunk.
    pub deletions: u32,
    /// The actual diff content of this hunk (unified diff format).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Information about a changed file in the diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    /// Path to the file (new path for renames).
    pub path: String,
    /// Old path (only set for renames/copies).
    pub old_path: Option<String>,
    /// Status of the file.
    pub status: FileStatus,
    /// Detected programming language.
    pub language: Option<String>,
    /// Lines added.
    pub additions: u32,
    /// Lines deleted.
    pub deletions: u32,
    /// Hunks in this file.
    pub hunks: Vec<DiffHunk>,
}

// =============================================================================
// Clean Stage Types
// =============================================================================

/// Result of the clean stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanResult {
    /// Run ID this result belongs to.
    pub run_id: String,
    /// Lint results.
    pub lint: Option<LintResult>,
    /// Format results.
    pub format: Option<FormatResult>,
    /// Secrets scan results.
    pub secrets: SecretsResult,
    /// Overall success status.
    pub success: bool,
    /// Timestamp when cleaning was performed.
    pub cleaned_at: i64,
}

/// Result of running the linter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintResult {
    /// Command that was run.
    pub command: String,
    /// Whether linting passed (no errors).
    pub passed: bool,
    /// Whether auto-fix was applied.
    pub auto_fixed: bool,
    /// Number of errors found.
    pub error_count: u32,
    /// Number of warnings found.
    pub warning_count: u32,
    /// Individual lint issues.
    pub issues: Vec<LintIssue>,
    /// Exit code of the linter.
    pub exit_code: i32,
    /// Raw output from the linter.
    pub raw_output: String,
}

/// A single lint issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintIssue {
    /// File path where the issue was found.
    pub file: String,
    /// Line number (1-based, if available).
    pub line: Option<u32>,
    /// Column number (1-based, if available).
    pub column: Option<u32>,
    /// Severity: "error" or "warning".
    pub severity: String,
    /// Rule ID or code (e.g., "no-unused-vars").
    pub rule: Option<String>,
    /// Human-readable message.
    pub message: String,
}

/// Result of running the formatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatResult {
    /// Command that was run.
    pub command: String,
    /// Whether formatting was applied.
    pub formatted: bool,
    /// Files that were reformatted (if available).
    pub files_changed: Vec<String>,
    /// Exit code of the formatter.
    pub exit_code: i32,
    /// Raw output from the formatter.
    pub raw_output: String,
}

/// Result of scanning for secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretsResult {
    /// Whether any secrets were found.
    pub found_secrets: bool,
    /// Individual secret findings.
    pub findings: Vec<SecretFinding>,
}

/// A potential secret found in the diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretFinding {
    /// File path where the secret was found.
    pub file: String,
    /// Line number where the secret was found.
    pub line: u32,
    /// Type of secret detected.
    pub secret_type: String,
    /// The matched pattern (redacted for safety).
    pub matched: String,
    /// Confidence level: "high", "medium", or "low".
    pub confidence: String,
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
