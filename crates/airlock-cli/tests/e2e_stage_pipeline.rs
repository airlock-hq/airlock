//! End-to-end tests for the workflow/job/step pipeline.
//!
//! These tests verify the full pipeline flow:
//! 1. Set up real git repos (upstream, working)
//! 2. Create runs with job and step results in the database
//! 3. Verify step execution, approval, and rejection workflows
//! 4. Verify run state derivation from step/job results
//! 5. Multi-workflow, parallel jobs, DAG execution

#[path = "e2e_stage_pipeline_tests/helpers.rs"]
mod helpers;

#[path = "e2e_stage_pipeline_tests/run_state.rs"]
mod run_state;

#[path = "e2e_stage_pipeline_tests/step_operations.rs"]
mod step_operations;

#[path = "e2e_stage_pipeline_tests/pipeline_flow.rs"]
mod pipeline_flow;

#[path = "e2e_stage_pipeline_tests/git_operations.rs"]
mod git_operations;

#[path = "e2e_stage_pipeline_tests/workflow_config.rs"]
mod workflow_config;

#[path = "e2e_stage_pipeline_tests/workflow_loading.rs"]
mod workflow_loading;
