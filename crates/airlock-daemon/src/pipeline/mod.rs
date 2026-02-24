//! Transformation pipeline implementation.
//!
//! ## Workflow/Job/Step Pipeline
//!
//! The pipeline executes user-defined jobs and steps:
//! 1. Create a single worktree at the head commit
//! 2. Execute jobs (potentially in parallel via DAG)
//! 3. Each job runs its steps sequentially
//! 4. Steps can pause for approval with `require-approval: true`
//! 5. Pipeline resumes after approval
//!
//! ## Key Components
//!
//! - [`executor`]: Core step execution engine - runs individual steps

pub mod executor;

// Re-export key types for convenience
pub use executor::{
    append_log_capped, build_stage_environment, create_run_artifacts_dir,
    execute_stage_with_log_callback, resolve_effective_base_sha, should_continue_pipeline,
    should_pause_for_approval, LogStreamCallback, StageEnvironmentParams,
};
