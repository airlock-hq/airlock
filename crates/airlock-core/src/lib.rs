//! Airlock Core Library
//!
//! This crate provides shared types, configuration, and utilities for the Airlock system.
//! It is used by the CLI, daemon, and desktop app.

/// ASCII art banner for Airlock, used in CLI output and git hooks.
pub const BANNER: &str = r#"
 ▗▄▖ ▗▄▄▄▖▗▄▄▖ ▗▖    ▗▄▖  ▗▄▄▖▗▖ ▗▖
▐▌ ▐▌  █  ▐▌ ▐▌▐▌   ▐▌ ▐▌▐▌   ▐▌▗▞▘
▐▛▀▜▌  █  ▐▛▀▚▖▐▌   ▐▌ ▐▌▐▌   ▐▛▚▖
▐▌ ▐▌▗▄█▄▖▐▌ ▐▌▐▙▄▄▖▝▚▄▞▘▝▚▄▄▖▐▌ ▐▌
"#;

/// Brand color for terminal output (ANSI 256-color index).
///
/// Matches the "orbital violet signal" from the design system (`HSL(250, 55%, 55%)`).
/// Color 98 (`#875fd7`) is the closest 256-color approximation, chosen for wide
/// terminal support and reasonable visibility on both light and dark backgrounds.
pub const BRAND_COLOR_256: u8 = 98;

pub mod agent;
pub mod config;
pub mod db;
pub mod error;
pub mod git;
pub mod gui;
pub mod init;
pub mod ipc;
pub mod jj;
pub mod patches;
pub mod paths;
pub mod provider;
pub mod service;
pub mod types;
pub mod worktree;

// --- Agent types ---
pub use agent::{
    create_adapter, try_extract_json, AgentAdapter, AgentEvent, AgentEventStream, AgentMessage,
    AgentRequest, AgentResult, AgentUsage, ClaudeCodeAdapter, CodexAdapter, ContentBlock,
    StreamCollector,
};

// --- Config types ---
pub use config::{
    branch_matches_trigger, filter_workflows_for_branch, load_global_config,
    load_workflows_from_disk, load_workflows_from_tree, parse_workflow_config, validate_job_dag,
    AgentGlobalConfig, AgentOptions, DagValidationError, GlobalConfig, JobConfig, OneOrMany,
    PushTrigger, TriggerConfig, WorkflowConfig,
};

// --- Database ---
pub use db::{job_status_to_string, step_status_to_string, string_to_job_status, Database};

// --- Error handling ---
pub use error::{AirlockError, Result};

// --- Git operations ---
pub use git::{
    compute_diff, find_effective_base_sha, hooks, show_file, DiffResult, RefUpdateType,
    DEFAULT_BRANCHES, EMPTY_TREE_SHA,
};

// --- Initialization ---
pub use init::{BYPASS_REMOTE, REPO_CONFIG_PATH};

// --- Paths & providers ---
pub use paths::AirlockPaths;
pub use provider::{check_provider_setup, detect_provider, ProviderCheck, ScmProvider};
pub use service::ServiceManager;

// --- Domain types ---
pub use types::{
    ApprovalMode, CleanResult, DependencyGraph, DiffAnalysis, DiffHunk, FileChange, FileStatus,
    FormatResult, GuidedTour, HunkDependencies, Intent, IntentCategory, IntentStatus, JobResult,
    JobStatus, LineAnnotation, LintIssue, LintResult, RefAnalysis, RefUpdate, Repo, Run,
    SecretFinding, SecretsResult, SplitAnalysis, SplitHunk, SplitIntent, StepDefinition,
    StepResult, StepStatus, SyncLog, TourResult, TourStep,
};

// --- Worktree management ---
pub use worktree::{
    create_run_worktree, is_valid_worktree, list_worktrees, remove_run_worktree, remove_worktree,
    reset_persistent_worktree,
};
