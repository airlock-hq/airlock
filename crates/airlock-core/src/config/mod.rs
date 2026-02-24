//! Configuration types for Airlock.
//!
//! This module provides all configuration types used throughout the Airlock system,
//! organized into submodules by concern:
//!
//! - `sync` - Sync and storage configuration (`SyncConfig`, `StorageConfig`)
//! - `global` - Global configuration file (`GlobalConfig`)
//! - `workflow` - Workflow configuration (`WorkflowConfig`, `JobConfig`, trigger filters)
//! - `loader` - Configuration file loading utilities

mod global;
mod loader;
mod sync;
pub mod workflow;

#[cfg(test)]
mod tests;

// Re-export all public types for convenience
pub use global::{AgentGlobalConfig, AgentOptions, GlobalConfig};
pub use loader::{
    filter_workflows_for_branch, load_global_config, load_workflows_from_disk,
    load_workflows_from_tree, parse_workflow_config,
};
pub use sync::{StorageConfig, SyncConfig};
pub use workflow::{
    branch_matches_trigger, validate_job_dag, DagValidationError, JobConfig, OneOrMany,
    PushTrigger, TriggerConfig, WorkflowConfig,
};
