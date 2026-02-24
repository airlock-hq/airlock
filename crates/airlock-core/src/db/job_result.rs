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

impl Database {
    /// Insert a new job result.
    pub fn insert_job_result(&self, job_result: &JobResult) -> Result<()> {
        let status_str = job_status_to_string(job_result.status);

        self.conn
            .execute(
                "INSERT INTO job_results (id, run_id, job_key, name, status, job_order, started_at, completed_at, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
                "SELECT id, run_id, job_key, name, status, job_order, started_at, completed_at, error
                 FROM job_results WHERE id = ?1",
                [id],
                |row| {
                    let status_str: String = row.get(4)?;
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        status_str,
                        row.get::<_, i32>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                        row.get::<_, Option<String>>(8)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| AirlockError::Database(format!("Failed to get job result: {}", e)))?;

        match result {
            Some((
                id,
                run_id,
                job_key,
                name,
                status_str,
                job_order,
                started_at,
                completed_at,
                error,
            )) => {
                let status = string_to_job_status(&status_str)?;
                Ok(Some(JobResult {
                    id,
                    run_id,
                    job_key,
                    name,
                    status,
                    job_order,
                    started_at,
                    completed_at,
                    error,
                }))
            }
            None => Ok(None),
        }
    }

    /// Get all job results for a run, ordered by job_order.
    pub fn get_job_results_for_run(&self, run_id: &str) -> Result<Vec<JobResult>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, run_id, job_key, name, status, job_order, started_at, completed_at, error
                 FROM job_results WHERE run_id = ?1 ORDER BY job_order ASC",
            )
            .map_err(|e| AirlockError::Database(format!("Failed to prepare statement: {}", e)))?;

        let rows = stmt
            .query_map([run_id], |row| {
                let status_str: String = row.get(4)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    status_str,
                    row.get::<_, i32>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            })
            .map_err(|e| AirlockError::Database(format!("Failed to query job results: {}", e)))?;

        let mut job_results = Vec::new();
        for row in rows {
            let (id, run_id, job_key, name, status_str, job_order, started_at, completed_at, error) =
                row.map_err(|e| AirlockError::Database(format!("Failed to read row: {}", e)))?;

            let status = string_to_job_status(&status_str)?;

            job_results.push(JobResult {
                id,
                run_id,
                job_key,
                name,
                status,
                job_order,
                started_at,
                completed_at,
                error,
            });
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
}
