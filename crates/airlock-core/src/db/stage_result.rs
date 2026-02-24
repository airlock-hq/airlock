//! StepResult CRUD operations.

use crate::error::{AirlockError, Result};
use crate::types::StepResult;
use rusqlite::{params, OptionalExtension};

use super::helpers::{step_status_to_string, string_to_step_status};
use super::Database;

impl Database {
    /// Insert a new step result.
    pub fn insert_step_result(&self, step_result: &StepResult) -> Result<()> {
        let status_str = step_status_to_string(step_result.status);

        self.conn
            .execute(
                "INSERT INTO step_results (id, run_id, job_id, name, status, step_order, exit_code, duration_ms, error, started_at, completed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    step_result.id,
                    step_result.run_id,
                    step_result.job_id,
                    step_result.name,
                    status_str,
                    step_result.step_order,
                    step_result.exit_code,
                    step_result.duration_ms,
                    step_result.error,
                    step_result.started_at,
                    step_result.completed_at,
                ],
            )
            .map_err(|e| AirlockError::Database(format!("Failed to insert step result: {}", e)))?;

        tracing::debug!("Inserted step result: {}", step_result.id);
        Ok(())
    }

    /// Get a step result by ID.
    pub fn get_step_result(&self, id: &str) -> Result<Option<StepResult>> {
        let result = self
            .conn
            .query_row(
                "SELECT id, run_id, job_id, name, status, step_order, exit_code, duration_ms, error, started_at, completed_at
                 FROM step_results WHERE id = ?1",
                [id],
                |row| {
                    Ok(StepResult {
                        id: row.get(0)?,
                        run_id: row.get(1)?,
                        job_id: row.get(2)?,
                        name: row.get(3)?,
                        status: Default::default(), // Placeholder, set below
                        step_order: row.get(5)?,
                        exit_code: row.get(6)?,
                        duration_ms: row.get(7)?,
                        error: row.get(8)?,
                        started_at: row.get(9)?,
                        completed_at: row.get(10)?,
                    })
                },
            )
            .optional()
            .map_err(|e| AirlockError::Database(format!("Failed to get step result: {}", e)))?;

        match result {
            Some(mut step_result) => {
                // Parse status string separately to handle error conversion
                let status_str: String = self
                    .conn
                    .query_row(
                        "SELECT status FROM step_results WHERE id = ?1",
                        [id],
                        |row| row.get(0),
                    )
                    .map_err(|e| {
                        AirlockError::Database(format!("Failed to get step result status: {}", e))
                    })?;
                step_result.status = string_to_step_status(&status_str)?;
                Ok(Some(step_result))
            }
            None => Ok(None),
        }
    }

    /// Get all step results for a run.
    pub fn get_step_results_for_run(&self, run_id: &str) -> Result<Vec<StepResult>> {
        self.query_step_results("run_id", run_id)
    }

    /// Get all step results for a specific job.
    pub fn get_step_results_for_job(&self, job_id: &str) -> Result<Vec<StepResult>> {
        self.query_step_results("job_id", job_id)
    }

    /// Shared query logic for fetching step results filtered by a column.
    fn query_step_results(
        &self,
        filter_column: &str,
        filter_value: &str,
    ) -> Result<Vec<StepResult>> {
        let sql = format!(
            "SELECT id, run_id, job_id, name, status, step_order, exit_code, duration_ms, error, started_at, completed_at \
             FROM step_results WHERE {} = ?1 ORDER BY step_order ASC",
            filter_column
        );

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| AirlockError::Database(format!("Failed to prepare statement: {}", e)))?;

        let rows = stmt
            .query_map([filter_value], |row| {
                let status_str: String = row.get(4)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    status_str,
                    row.get::<_, i32>(5)?,
                    row.get::<_, Option<i32>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                ))
            })
            .map_err(|e| AirlockError::Database(format!("Failed to query step results: {}", e)))?;

        let mut step_results = Vec::new();
        for row in rows {
            let (
                id,
                run_id,
                job_id,
                name,
                status_str,
                step_order,
                exit_code,
                duration_ms,
                error,
                started_at,
                completed_at,
            ) = row.map_err(|e| AirlockError::Database(format!("Failed to read row: {}", e)))?;

            let status = string_to_step_status(&status_str)?;

            step_results.push(StepResult {
                id,
                run_id,
                job_id,
                name,
                status,
                step_order,
                exit_code,
                duration_ms,
                error,
                started_at,
                completed_at,
            });
        }

        Ok(step_results)
    }

    /// Update a step result.
    pub fn update_step_result(&self, step_result: &StepResult) -> Result<()> {
        let status_str = step_status_to_string(step_result.status);

        let rows_affected = self
            .conn
            .execute(
                "UPDATE step_results SET status = ?1, exit_code = ?2, duration_ms = ?3, error = ?4, started_at = ?5, completed_at = ?6
                 WHERE id = ?7",
                params![
                    status_str,
                    step_result.exit_code,
                    step_result.duration_ms,
                    step_result.error,
                    step_result.started_at,
                    step_result.completed_at,
                    step_result.id,
                ],
            )
            .map_err(|e| AirlockError::Database(format!("Failed to update step result: {}", e)))?;

        if rows_affected == 0 {
            return Err(AirlockError::NotFound(
                "StepResult".into(),
                step_result.id.clone(),
            ));
        }

        tracing::debug!("Updated step result: {}", step_result.id);
        Ok(())
    }

    /// Delete all step results for a run.
    pub fn delete_step_results_for_run(&self, run_id: &str) -> Result<u32> {
        let rows_affected = self
            .conn
            .execute("DELETE FROM step_results WHERE run_id = ?1", [run_id])
            .map_err(|e| {
                AirlockError::Database(format!("Failed to delete step results for run: {}", e))
            })?;

        tracing::debug!("Deleted {} step results for run: {}", rows_affected, run_id);
        Ok(rows_affected as u32)
    }
}
