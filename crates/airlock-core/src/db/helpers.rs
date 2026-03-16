//! Helper functions for enum serialization in the database.
//!
//! Status enums ([`StepStatus`], [`JobStatus`]) share the same string
//! representation in SQLite. The [`status_db_conversion!`] macro generates
//! the to-string / from-string helpers to keep the mapping in one place
//! per enum.

use crate::error::{AirlockError, Result};
use crate::types::{JobStatus, StepStatus};

/// Generate a pair of conversion functions (`<prefix>_to_string` and
/// `string_to_<prefix>`) for a status enum whose variants map to fixed
/// `&'static str` values stored in the database.
macro_rules! status_db_conversion {
    (
        $enum_ty:ty, $label:literal,
        $to_fn:ident, $from_fn:ident,
        $( $variant:ident => $str:literal ),+ $(,)?
    ) => {
        pub fn $to_fn(status: $enum_ty) -> &'static str {
            match status {
                $( <$enum_ty>::$variant => $str, )+
            }
        }

        pub fn $from_fn(s: &str) -> Result<$enum_ty> {
            match s {
                $( $str => Ok(<$enum_ty>::$variant), )+
                _ => Err(AirlockError::Database(format!(
                    "Unknown {} status: {s}", $label
                ))),
            }
        }
    };
}

// ── StepStatus ──────────────────────────────────────────────────────────────

status_db_conversion!(
    StepStatus, "step",
    step_status_to_string, string_to_step_status,
    Pending          => "pending",
    Running          => "running",
    Passed           => "passed",
    Failed           => "failed",
    Skipped          => "skipped",
    AwaitingApproval => "awaiting_approval",
);

// ── JobStatus ───────────────────────────────────────────────────────────────

status_db_conversion!(
    JobStatus, "job",
    job_status_to_string, string_to_job_status,
    Pending          => "pending",
    Running          => "running",
    Passed           => "passed",
    Failed           => "failed",
    Skipped          => "skipped",
    AwaitingApproval => "awaiting_approval",
);
