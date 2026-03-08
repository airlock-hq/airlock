//! `airlock exec` command implementations.
//!
//! These commands are designed to be called from within pipeline stages.
//! They read configuration from AIRLOCK_* environment variables set by the stage executor.
//!
//! Available subcommands:
//! - `agent` - Run prompts through Claude Code CLI
//! - `freeze` - Apply patches and create checkpoint commit
//! - `json` - JSON helper for extracting/modifying JSON data

mod agent;
mod await_approval;
mod freeze;
mod json;
mod push;

#[cfg(test)]
mod env;
#[cfg(test)]
mod helpers;
#[cfg(test)]
mod tests;

pub use agent::agent;
pub use await_approval::await_approval;
pub use freeze::freeze;
pub use json::{json, JsonArgs};
pub use push::push;
