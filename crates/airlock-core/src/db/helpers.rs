//! Helper functions for enum serialization in the database.

use crate::error::{AirlockError, Result};
use crate::types::StepStatus;

/// Convert StepStatus to string for database storage.
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

/// Convert string from database to StepStatus.
pub fn string_to_step_status(s: &str) -> Result<StepStatus> {
    match s {
        "pending" => Ok(StepStatus::Pending),
        "running" => Ok(StepStatus::Running),
        "passed" => Ok(StepStatus::Passed),
        "failed" => Ok(StepStatus::Failed),
        "skipped" => Ok(StepStatus::Skipped),
        "awaiting_approval" => Ok(StepStatus::AwaitingApproval),
        _ => Err(AirlockError::Database(format!(
            "Unknown step status: {}",
            s
        ))),
    }
}
