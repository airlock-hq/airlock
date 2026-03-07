//! JobResult CRUD operations.

use crate::error::{AirlockError, Result};
use crate::types::{JobResult, JobStatus};
use rusqlite::{params, OptionalExtension};

use super::Database;

/// Convert JobStatus to string for database storage.
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

/// Convert string from database to JobStatus.
pub fn string_to_job_status(s: &str) -> Result<JobStatus> {
    match s {
        "pending" => Ok(JobStatus::Pending),
        "running" => Ok(JobStatus::Running),
        "passed" => Ok(JobStatus::Passed),
        "failed" => Ok(JobStatus::Failed),
        "skipped" => Ok(JobStatus::Skipped),
        "awaiting_approval" => Ok(JobStatus::AwaitingApproval),
        _ => Err(AirlockError::Database(format!("Unknown job status: {}", s))),
    }
}

/// Internal row type for reading job results from the database.
struct JobResultRow {
    id: String,
    run_id: String,
    job_key: String,
    name: Option<String>,
    status_str: String,
    job_order: i32,
    started_at: Option<i64>,
    completed_at: Option<i64>,
    error: Option<String>,
    worktree_path: Option<String>,
}

impl JobResultRow {
    fn into_job_result(self) -> Result<JobResult> {
        let status = string_to_job_status(&self.status_str)?;
        Ok(JobResult {
            id: self.id,
            run_id: self.run_id,
            job_key: self.job_key,
            name: self.name,
            status,
            job_order: self.job_order,
            started_at: self.started_at,
            completed_at: self.completed_at,
            error: self.error,
            worktree_path: self.worktree_path,
        })
    }
}

impl Database {
    /// Insert a new job result.
    pub fn insert_job_result(&self, job_result: &JobResult) -> Result<()> {
        let status_str = job_status_to_string(job_result.status);

        self.conn
            .execute(
                "INSERT INTO job_results (id, run_id, job_key, name, status, job_order, started_at, completed_at, error, worktree_path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    job_result.id,
                    job_result.run_id,
                    job_result.job_key,
                    job_result.name,
                    status_str,
                    job_result.job_order,
                    job_result.started_at,
                    job_result.completed_at,
                    job_result.error,
                    job_result.worktree_path,
                ],
            )
            .map_err(|e| AirlockError::Database(format!("Failed to insert job result: {}", e)))?;

        tracing::debug!("Inserted job result: {}", job_result.id);
        Ok(())
    }

    /// Get a job result by ID.
    pub fn get_job_result(&self, id: &str) -> Result<Option<JobResult>> {
        let result = self
            .conn
            .query_row(
                "SELECT id, run_id, job_key, name, status, job_order, started_at, completed_at, error, worktree_path
                 FROM job_results WHERE id = ?1",
                [id],
                |row| {
                    Ok(JobResultRow {
                        id: row.get(0)?,
                        run_id: row.get(1)?,
                        job_key: row.get(2)?,
                        name: row.get(3)?,
                        status_str: row.get(4)?,
                        job_order: row.get(5)?,
                        started_at: row.get(6)?,
                        completed_at: row.get(7)?,
                        error: row.get(8)?,
                        worktree_path: row.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(|e| AirlockError::Database(format!("Failed to get job result: {}", e)))?;

        match result {
            Some(row) => Ok(Some(row.into_job_result()?)),
            None => Ok(None),
        }
    }

    /// Get all job results for a run, ordered by job_order.
    pub fn get_job_results_for_run(&self, run_id: &str) -> Result<Vec<JobResult>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, run_id, job_key, name, status, job_order, started_at, completed_at, error, worktree_path
                 FROM job_results WHERE run_id = ?1 ORDER BY job_order ASC",
            )
            .map_err(|e| AirlockError::Database(format!("Failed to prepare statement: {}", e)))?;

        let rows = stmt
            .query_map([run_id], |row| {
                Ok(JobResultRow {
                    id: row.get(0)?,
                    run_id: row.get(1)?,
                    job_key: row.get(2)?,
                    name: row.get(3)?,
                    status_str: row.get(4)?,
                    job_order: row.get(5)?,
                    started_at: row.get(6)?,
                    completed_at: row.get(7)?,
                    error: row.get(8)?,
                    worktree_path: row.get(9)?,
                })
            })
            .map_err(|e| AirlockError::Database(format!("Failed to query job results: {}", e)))?;

        let mut job_results = Vec::new();
        for row in rows {
            let row =
                row.map_err(|e| AirlockError::Database(format!("Failed to read row: {}", e)))?;
            job_results.push(row.into_job_result()?);
        }

        Ok(job_results)
    }

    /// Update a job result's status and timestamps.
    pub fn update_job_status(
        &self,
        id: &str,
        status: JobStatus,
        started_at: Option<i64>,
        completed_at: Option<i64>,
        error: Option<&str>,
    ) -> Result<()> {
        let status_str = job_status_to_string(status);

        let rows_affected = self
            .conn
            .execute(
                "UPDATE job_results SET status = ?1, started_at = COALESCE(?2, started_at), completed_at = ?3, error = ?4
                 WHERE id = ?5",
                params![status_str, started_at, completed_at, error, id],
            )
            .map_err(|e| {
                AirlockError::Database(format!("Failed to update job status: {}", e))
            })?;

        if rows_affected == 0 {
            return Err(AirlockError::NotFound("JobResult".into(), id.into()));
        }

        tracing::debug!("Updated job result status: {} -> {}", id, status_str);
        Ok(())
    }

    /// Conditionally update a job result's status — only if the current status matches `expected`.
    ///
    /// Returns `true` if the update was applied, `false` if the status had already changed.
    pub fn update_job_status_if(
        &self,
        id: &str,
        expected: JobStatus,
        new_status: JobStatus,
        completed_at: Option<i64>,
        error: Option<&str>,
    ) -> Result<bool> {
        let expected_str = job_status_to_string(expected);
        let new_str = job_status_to_string(new_status);

        let rows_affected = self
            .conn
            .execute(
                "UPDATE job_results SET status = ?1, completed_at = ?2, error = ?3
                 WHERE id = ?4 AND status = ?5",
                params![new_str, completed_at, error, id, expected_str],
            )
            .map_err(|e| {
                AirlockError::Database(format!("Failed to conditionally update job status: {}", e))
            })?;

        if rows_affected > 0 {
            tracing::debug!(
                "Conditionally updated job status: {} -> {} (was {})",
                id,
                new_str,
                expected_str
            );
        } else {
            tracing::debug!(
                "Conditional update skipped for job {}: status was no longer {}",
                id,
                expected_str
            );
        }

        Ok(rows_affected > 0)
    }

    /// Delete all job results for a run.
    pub fn delete_job_results_for_run(&self, run_id: &str) -> Result<u32> {
        let rows_affected = self
            .conn
            .execute("DELETE FROM job_results WHERE run_id = ?1", [run_id])
            .map_err(|e| {
                AirlockError::Database(format!("Failed to delete job results for run: {}", e))
            })?;

        tracing::debug!("Deleted {} job results for run: {}", rows_affected, run_id);
        Ok(rows_affected as u32)
    }

    /// Get all AwaitingApproval jobs that have a worktree_path, joined with their repo_id.
    ///
    /// Used by worktree pool initialization to mark in-use slots after a restart.
    pub fn get_awaiting_approval_jobs_with_worktrees(
        &self,
    ) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT r.repo_id, j.job_key, j.worktree_path
                 FROM job_results j
                 JOIN runs r ON j.run_id = r.id
                 WHERE j.status = 'awaiting_approval' AND j.worktree_path IS NOT NULL",
            )
            .map_err(|e| AirlockError::Database(format!("Failed to prepare statement: {}", e)))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| {
                AirlockError::Database(format!("Failed to query awaiting approval jobs: {}", e))
            })?;

        let mut results = Vec::new();
        for row in rows {
            let row =
                row.map_err(|e| AirlockError::Database(format!("Failed to read row: {}", e)))?;
            results.push(row);
        }

        Ok(results)
    }

    /// Update the worktree_path for a job result.
    pub fn update_job_worktree_path(&self, id: &str, worktree_path: &str) -> Result<()> {
        let rows_affected = self
            .conn
            .execute(
                "UPDATE job_results SET worktree_path = ?1 WHERE id = ?2",
                params![worktree_path, id],
            )
            .map_err(|e| {
                AirlockError::Database(format!("Failed to update job worktree_path: {}", e))
            })?;

        if rows_affected == 0 {
            return Err(AirlockError::NotFound("JobResult".into(), id.into()));
        }

        tracing::debug!("Updated job {} worktree_path to {}", id, worktree_path);
        Ok(())
    }
}
