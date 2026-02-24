//! Artifact CLI commands.
//!
//! These commands are used by pipeline stages to produce Push Request artifacts:
//! - `airlock artifact content` - Add markdown content (e.g., summaries, reports)
//! - `airlock artifact comment` - Add code review comments
//! - `airlock artifact patch` - Capture changes as reviewable patches
//!
//! All commands require the `$AIRLOCK_ARTIFACTS` environment variable to be set,
//! which is automatically done when running inside a pipeline stage.

pub mod comment;
pub mod content;
pub mod patch;

pub use comment::comment;
pub use content::content;
pub use patch::patch;
