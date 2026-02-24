//! Shared run lookup utilities for CLI commands.

use anyhow::{Context, Result};

use airlock_core::Database;

/// Find a run by ID or ID prefix.
pub fn find_run_by_prefix(db: &Database, prefix: &str) -> Result<airlock_core::Run> {
    // First try exact match
    if let Some(run) = db.get_run(prefix).context("Failed to query run")? {
        return Ok(run);
    }

    // Try to find by prefix in all repos
    let repos = db.list_repos().context("Failed to list repos")?;

    let mut matches = Vec::new();
    for repo in &repos {
        let runs = db
            .list_runs(&repo.id, Some(100))
            .context("Failed to list runs")?;
        for run in runs {
            if run.id.starts_with(prefix) {
                matches.push(run);
            }
        }
    }

    match matches.len() {
        0 => anyhow::bail!("No run found with ID starting with '{}'", prefix),
        1 => Ok(matches.remove(0)),
        _ => {
            let ids: Vec<_> = matches.iter().map(|r| r.id.as_str()).collect();
            anyhow::bail!(
                "Ambiguous run ID prefix '{}'. Matches: {}",
                prefix,
                ids.join(", ")
            )
        }
    }
}
