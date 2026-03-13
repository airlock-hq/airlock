//! Repository CRUD operations.

use crate::error::{AirlockError, Result};
use crate::types::Repo;
use rusqlite::params;
use std::path::Path;

use super::Database;

impl Database {
    /// Insert a new repository.
    pub fn insert_repo(&self, repo: &Repo) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO repos (id, working_path, upstream_url, gate_path, last_sync, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    repo.id,
                    repo.working_path.to_string_lossy(),
                    repo.upstream_url,
                    repo.gate_path.to_string_lossy(),
                    repo.last_sync,
                    repo.created_at,
                ],
            )
            .map_err(|e| AirlockError::Database(format!("Failed to insert repo: {e}")))?;

        tracing::debug!("Inserted repo: {}", repo.id);
        Ok(())
    }

    /// Get a repository by ID.
    pub fn get_repo(&self, id: &str) -> Result<Option<Repo>> {
        use rusqlite::OptionalExtension;

        let result = self
            .conn
            .query_row(
                "SELECT id, working_path, upstream_url, gate_path, last_sync, created_at
                 FROM repos WHERE id = ?1",
                [id],
                |row| {
                    Ok(Repo {
                        id: row.get(0)?,
                        working_path: std::path::PathBuf::from(row.get::<_, String>(1)?),
                        upstream_url: row.get(2)?,
                        gate_path: std::path::PathBuf::from(row.get::<_, String>(3)?),
                        last_sync: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(|e| AirlockError::Database(format!("Failed to get repo: {e}")))?;

        Ok(result)
    }

    /// Get a repository by working path.
    pub fn get_repo_by_path(&self, working_path: &Path) -> Result<Option<Repo>> {
        use rusqlite::OptionalExtension;

        let path_str = working_path.to_string_lossy();
        let result = self
            .conn
            .query_row(
                "SELECT id, working_path, upstream_url, gate_path, last_sync, created_at
                 FROM repos WHERE working_path = ?1",
                [path_str.as_ref()],
                |row| {
                    Ok(Repo {
                        id: row.get(0)?,
                        working_path: std::path::PathBuf::from(row.get::<_, String>(1)?),
                        upstream_url: row.get(2)?,
                        gate_path: std::path::PathBuf::from(row.get::<_, String>(3)?),
                        last_sync: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(|e| AirlockError::Database(format!("Failed to get repo by path: {e}")))?;

        Ok(result)
    }

    /// List all repositories.
    pub fn list_repos(&self) -> Result<Vec<Repo>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, working_path, upstream_url, gate_path, last_sync, created_at
                 FROM repos ORDER BY created_at DESC",
            )
            .map_err(|e| AirlockError::Database(format!("Failed to prepare statement: {e}")))?;

        let repos = stmt
            .query_map([], |row| {
                Ok(Repo {
                    id: row.get(0)?,
                    working_path: std::path::PathBuf::from(row.get::<_, String>(1)?),
                    upstream_url: row.get(2)?,
                    gate_path: std::path::PathBuf::from(row.get::<_, String>(3)?),
                    last_sync: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .map_err(|e| AirlockError::Database(format!("Failed to query repos: {e}")))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| AirlockError::Database(format!("Failed to collect repos: {e}")))?;

        Ok(repos)
    }

    /// Update a repository's last sync timestamp.
    pub fn update_repo_last_sync(&self, id: &str, last_sync: i64) -> Result<()> {
        let rows_affected = self
            .conn
            .execute(
                "UPDATE repos SET last_sync = ?1 WHERE id = ?2",
                params![last_sync, id],
            )
            .map_err(|e| AirlockError::Database(format!("Failed to update repo last_sync: {e}")))?;

        if rows_affected == 0 {
            return Err(AirlockError::NotFound("Repo".into(), id.into()));
        }

        Ok(())
    }

    /// Delete a repository and all associated data.
    pub fn delete_repo(&self, id: &str) -> Result<()> {
        let rows_affected = self
            .conn
            .execute("DELETE FROM repos WHERE id = ?1", [id])
            .map_err(|e| AirlockError::Database(format!("Failed to delete repo: {e}")))?;

        if rows_affected == 0 {
            return Err(AirlockError::NotFound("Repo".into(), id.into()));
        }

        tracing::debug!("Deleted repo: {}", id);
        Ok(())
    }
}
