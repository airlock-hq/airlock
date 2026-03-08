//! `airlock exec await` — request human approval from within a pipeline step.
//!
//! Writes a `.awaiting` marker file in the step's logs directory.  The step
//! exits 0 normally; the executor detects the marker after completion and
//! transitions the step to `AwaitingApproval`.
//!
//! Usage:
//!   airlock exec await                          # default message
//!   airlock exec await "Tests failed, review"   # custom message

use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing::info;

/// Execute the `await` command.
///
/// Writes a `.awaiting` marker and exits 0.  The executor picks up the marker
/// and pauses the pipeline using the existing approval infrastructure.
pub async fn await_approval(message: Option<String>) -> Result<()> {
    let stage_result = std::env::var("AIRLOCK_STAGE_RESULT").context(
        "AIRLOCK_STAGE_RESULT not set. This command must be run within a pipeline stage.",
    )?;

    let logs_dir = PathBuf::from(&stage_result)
        .parent()
        .map(|p| p.to_path_buf())
        .context("Could not determine logs directory from AIRLOCK_STAGE_RESULT")?;

    // Write marker file in the step's logs directory (per-job, per-step — no conflicts)
    let marker_path = logs_dir.join(".awaiting");
    let marker = serde_json::json!({
        "message": message.as_deref().unwrap_or("Awaiting human approval"),
    });
    std::fs::write(&marker_path, serde_json::to_string_pretty(&marker)?)
        .with_context(|| format!("Failed to write .awaiting marker to {:?}", marker_path))?;

    let msg = message.as_deref().unwrap_or("Awaiting human approval");
    info!("{}", msg);
    eprintln!("Awaiting human approval: {}", msg);

    Ok(())
}
