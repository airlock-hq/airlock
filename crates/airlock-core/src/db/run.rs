//! Run CRUD operations.

use crate::error::{AirlockError, Result};
use crate::types::{RefUpdate, Run};
use rusqlite::{params, OptionalExtension};

use super::Database;

/// Helper to map a row to a Run struct.
/// Column order: id, repo_id, branch, base_sha, head_sha, current_stage, error, superseded, workflow_file, workflow_name, created_at, updated_at
fn row_to_run(row: &rusqlite::Row) -> rusqlite::Result<Run> {
    let branch: String = row.get(2)?;
    let base_sha: String = row.get(3)?;
    let head_sha: String = row.get(4)?;
    // Reconstruct ref_name with refs/heads/ prefix for proper matching
    let ref_name = if branch.is_empty() {
        branch.clone()
    } else {
        format!("refs/heads/{}", branch)
    };
    Ok(Run {
        id: row.get(0)?,
        repo_id: row.get(1)?,
        ref_updates: vec![RefUpdate {
            ref_name,
            old_sha: base_sha.clone(),
            new_sha: head_sha.clone(),
        }],
        branch,
        base_sha,
        head_sha,
        current_step: row.get(5)?,
        error: row.get(6)?,
        superseded: row.get(7)?,
        workflow_file: row
            .get::<_, Option<String>>(8)?
            .unwrap_or_else(|| "main.yml".to_string()),
        workflow_name: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

const SELECT_RUN_COLUMNS: &str =
    "id, repo_id, branch, base_sha, head_sha, current_stage, error, superseded, workflow_file, workflow_name, created_at, updated_at";

impl Database {
    /// Insert a new run.
    pub fn insert_run(&self, run: &Run) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO runs (id, repo_id, branch, base_sha, head_sha, current_stage, error, superseded, workflow_file, workflow_name, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    run.id,
                    run.repo_id,
                    run.branch,
                    run.base_sha,
                    run.head_sha,
                    run.current_step,
                    run.error,
                    run.superseded,
                    run.workflow_file,
                    run.workflow_name,
                    run.created_at,
                    run.updated_at,
                ],
            )
            .map_err(|e| AirlockError::Database(format!("Failed to insert run: {}", e)))?;

        tracing::debug!("Inserted run: {}", run.id);
        Ok(())
    }

    /// Get a run by ID.
    pub fn get_run(&self, id: &str) -> Result<Option<Run>> {
        let query = format!("SELECT {} FROM runs WHERE id = ?1", SELECT_RUN_COLUMNS);
        let result = self
            .conn
            .query_row(&query, [id], row_to_run)
            .optional()
            .map_err(|e| AirlockError::Database(format!("Failed to get run: {}", e)))?;

        Ok(result)
    }

    /// List runs for a repository.
    pub fn list_runs(&self, repo_id: &str, limit: Option<u32>) -> Result<Vec<Run>> {
        let limit = limit.unwrap_or(100);
        let query = format!(
            "SELECT {} FROM runs WHERE repo_id = ?1 ORDER BY created_at DESC LIMIT ?2",
            SELECT_RUN_COLUMNS
        );
        let mut stmt = self
            .conn
            .prepare(&query)
            .map_err(|e| AirlockError::Database(format!("Failed to prepare statement: {}", e)))?;

        let runs = stmt
            .query_map(params![repo_id, limit], row_to_run)
            .map_err(|e| AirlockError::Database(format!("Failed to query runs: {}", e)))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AirlockError::Database(format!("Failed to collect runs: {}", e)))?;

        Ok(runs)
    }

    /// Update a run's current step.
    pub fn update_run_current_step(&self, id: &str, current_step: Option<&str>) -> Result<()> {
        let updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let rows_affected = self
            .conn
            .execute(
                "UPDATE runs SET current_stage = ?1, updated_at = ?2 WHERE id = ?3",
                params![current_step, updated_at, id],
            )
            .map_err(|e| {
                AirlockError::Database(format!("Failed to update run current_step: {}", e))
            })?;

        if rows_affected == 0 {
            return Err(AirlockError::NotFound("Run".into(), id.into()));
        }

        Ok(())
    }

    /// Update a run's error field.
    pub fn update_run_error(&self, id: &str, error: Option<&str>) -> Result<()> {
        let updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let rows_affected = self
            .conn
            .execute(
                "UPDATE runs SET error = ?1, updated_at = ?2 WHERE id = ?3",
                params![error, updated_at, id],
            )
            .map_err(|e| AirlockError::Database(format!("Failed to update run error: {}", e)))?;

        if rows_affected == 0 {
            return Err(AirlockError::NotFound("Run".into(), id.into()));
        }

        Ok(())
    }

    /// Update a run's head_sha (e.g., after applying patches).
    pub fn update_run_head_sha(&self, id: &str, head_sha: &str) -> Result<()> {
        let updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let rows_affected = self
            .conn
            .execute(
                "UPDATE runs SET head_sha = ?1, updated_at = ?2 WHERE id = ?3",
                params![head_sha, updated_at, id],
            )
            .map_err(|e| AirlockError::Database(format!("Failed to update run head_sha: {}", e)))?;

        if rows_affected == 0 {
            return Err(AirlockError::NotFound("Run".into(), id.into()));
        }

        Ok(())
    }

    /// Mark a run as superseded by a newer push.
    pub fn mark_run_superseded(&self, id: &str) -> Result<()> {
        let updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let rows_affected = self
            .conn
            .execute(
                "UPDATE runs SET superseded = 1, error = ?1, updated_at = ?2 WHERE id = ?3",
                params!["Superseded by newer push", updated_at, id],
            )
            .map_err(|e| {
                AirlockError::Database(format!("Failed to mark run as superseded: {}", e))
            })?;

        if rows_affected == 0 {
            return Err(AirlockError::NotFound("Run".into(), id.into()));
        }

        Ok(())
    }

    /// Get runs that have non-completed stages.
    ///
    /// Active runs are those where at least one stage is not in a final state
    /// (i.e., has stages that are pending, running, or awaiting_approval).
    pub fn list_active_runs(&self, repo_id: &str) -> Result<Vec<Run>> {
        // First get all runs for the repo
        let runs = self.list_runs(repo_id, None)?;

        // Filter to those with non-final stages (excluding superseded runs)
        let mut active_runs = Vec::new();
        for run in runs {
            if run.is_superseded() {
                continue;
            }
            let jobs = self.get_job_results_for_run(&run.id)?;
            if jobs.is_empty() {
                // No jobs yet means the run is pending
                active_runs.push(run);
            } else if run.is_running_from_jobs(&jobs) {
                active_runs.push(run);
            }
        }

        Ok(active_runs)
    }

    /// List all runs across all repos.
    ///
    /// This is useful for daemon startup to check for orphaned runs.
    pub fn list_all_runs(&self, limit: Option<u32>) -> Result<Vec<Run>> {
        let limit = limit.unwrap_or(100);
        let query = format!(
            "SELECT {} FROM runs ORDER BY created_at DESC LIMIT ?1",
            SELECT_RUN_COLUMNS
        );
        let mut stmt = self
            .conn
            .prepare(&query)
            .map_err(|e| AirlockError::Database(format!("Failed to prepare statement: {}", e)))?;

        let runs = stmt
            .query_map([limit], row_to_run)
            .map_err(|e| AirlockError::Database(format!("Failed to query runs: {}", e)))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AirlockError::Database(format!("Failed to collect runs: {}", e)))?;

        Ok(runs)
    }

    /// Compute the derived status string for a run from its job results.
    ///
    /// This is a convenience method that wraps `get_job_results_for_run` + `Run::derived_status_from_jobs()`.
    pub fn compute_run_status(&self, run: &Run) -> Result<String> {
        let jobs = self.get_job_results_for_run(&run.id)?;
        Ok(run.derived_status_from_jobs(&jobs).to_string())
    }

    /// Delete a run and all associated job/step results.
    pub fn delete_run(&self, id: &str) -> Result<()> {
        let rows_affected = self
            .conn
            .execute("DELETE FROM runs WHERE id = ?1", [id])
            .map_err(|e| AirlockError::Database(format!("Failed to delete run: {}", e)))?;

        if rows_affected == 0 {
            return Err(AirlockError::NotFound("Run".into(), id.into()));
        }

        tracing::debug!("Deleted run: {}", id);
        Ok(())
    }
}
