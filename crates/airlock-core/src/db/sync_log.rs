//! SyncLog operations.

use crate::error::{AirlockError, Result};
use crate::types::SyncLog;
use rusqlite::{params, OptionalExtension};

use super::Database;

impl Database {
    /// Insert a new sync log entry.
    pub fn insert_sync_log(&self, sync_log: &SyncLog) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO sync_log (id, repo_id, success, error, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    sync_log.id,
                    sync_log.repo_id,
                    sync_log.success as i32,
                    sync_log.error,
                    sync_log.synced_at,
                ],
            )
            .map_err(|e| AirlockError::Database(format!("Failed to insert sync log: {e}")))?;

        tracing::debug!("Inserted sync log: {}", sync_log.id);
        Ok(())
    }

    /// Get the latest sync log for a repository.
    pub fn get_latest_sync_log(&self, repo_id: &str) -> Result<Option<SyncLog>> {
        let result = self
            .conn
            .query_row(
                "SELECT id, repo_id, success, error, synced_at
                 FROM sync_log WHERE repo_id = ?1 ORDER BY synced_at DESC LIMIT 1",
                [repo_id],
                |row| {
                    Ok(SyncLog {
                        id: row.get(0)?,
                        repo_id: row.get(1)?,
                        success: row.get::<_, i32>(2)? != 0,
                        error: row.get(3)?,
                        synced_at: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(|e| AirlockError::Database(format!("Failed to get latest sync log: {e}")))?;

        Ok(result)
    }

    /// List sync logs for a repository.
    pub fn list_sync_logs(&self, repo_id: &str, limit: Option<u32>) -> Result<Vec<SyncLog>> {
        let limit = limit.unwrap_or(100);
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, repo_id, success, error, synced_at
                 FROM sync_log WHERE repo_id = ?1 ORDER BY synced_at DESC LIMIT ?2",
            )
            .map_err(|e| AirlockError::Database(format!("Failed to prepare statement: {e}")))?;

        let logs = stmt
            .query_map(params![repo_id, limit], |row| {
                Ok(SyncLog {
                    id: row.get(0)?,
                    repo_id: row.get(1)?,
                    success: row.get::<_, i32>(2)? != 0,
                    error: row.get(3)?,
                    synced_at: row.get(4)?,
                })
            })
            .map_err(|e| AirlockError::Database(format!("Failed to query sync logs: {e}")))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AirlockError::Database(format!("Failed to collect sync logs: {e}")))?;

        Ok(logs)
    }

    /// Delete old sync logs, keeping only the most recent entries.
    pub fn cleanup_sync_logs(&self, repo_id: &str, keep_count: u32) -> Result<u32> {
        let deleted = self
            .conn
            .execute(
                "DELETE FROM sync_log WHERE repo_id = ?1 AND id NOT IN (
                    SELECT id FROM sync_log WHERE repo_id = ?1 ORDER BY synced_at DESC LIMIT ?2
                )",
                params![repo_id, keep_count],
            )
            .map_err(|e| AirlockError::Database(format!("Failed to cleanup sync logs: {e}")))?;

        Ok(deleted as u32)
    }
}
