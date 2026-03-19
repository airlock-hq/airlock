//! Helper functions for status enum serialization in the database.
//!
//! All status-to-string and string-to-status conversion functions live here
//! to keep the pattern co-located and consistent.

use crate::error::{AirlockError, Result};
use crate::types::{JobStatus, StepStatus};

pub fn job_status_to_string(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Pending => "pending",
        JobStatus::Running => "running",
        JobStatus::Passed => "passed",
        JobStatus::Failed => "failed",
        JobStatus::Skipped => "skipped",
        JobStatus::AwaitingApproval => "awaiting_approval",
    }
}

pub fn string_to_job_status(s: &str) -> Result<JobStatus> {
    match s {
        "pending" => Ok(JobStatus::Pending),
        "running" => Ok(JobStatus::Running),
        "passed" => Ok(JobStatus::Passed),
        "failed" => Ok(JobStatus::Failed),
        "skipped" => Ok(JobStatus::Skipped),
        "awaiting_approval" => Ok(JobStatus::AwaitingApproval),
        _ => Err(AirlockError::Database(format!("Unknown job status: {s}"))),
    }
}

pub fn step_status_to_string(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Pending => "pending",
        StepStatus::Running => "running",
        StepStatus::Passed => "passed",
        StepStatus::Failed => "failed",
        StepStatus::Skipped => "skipped",
        StepStatus::AwaitingApproval => "awaiting_approval",
    }
}

pub fn string_to_step_status(s: &str) -> Result<StepStatus> {
    match s {
        "pending" => Ok(StepStatus::Pending),
        "running" => Ok(StepStatus::Running),
        "passed" => Ok(StepStatus::Passed),
        "failed" => Ok(StepStatus::Failed),
        "skipped" => Ok(StepStatus::Skipped),
        "awaiting_approval" => Ok(StepStatus::AwaitingApproval),
        _ => Err(AirlockError::Database(format!("Unknown step status: {s}"))),
    }
}
