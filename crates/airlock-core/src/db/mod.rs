//! Database operations for Airlock state management.
//!
//! This module provides SQLite-based persistence for repositories, runs, step results,
//! and sync logs. It uses rusqlite for database operations.

mod helpers;
mod job_result;
mod repo;
mod run;
mod schema;
mod stage_result;
mod sync_log;

#[cfg(test)]
mod tests;

// Re-export step status helpers
pub use helpers::{step_status_to_string, string_to_step_status};
// Re-export job status helpers
pub use job_result::{job_status_to_string, string_to_job_status};

use crate::error::{AirlockError, Result};
use crate::paths::AirlockPaths;
use rusqlite::Connection;
use std::path::Path;

/// Current database schema version for migrations.
/// Version 4: Stage-based pipeline (removed intents, added stage_results)
/// Version 5: Added stage_order column to stage_results for correct ordering
/// Version 6: Removed artifacts_path column (artifacts now at run level)
/// Version 7: Added superseded column to runs
/// Version 8: Add workflow tracking to runs, add job_results table, replace stage_results with step_results
const SCHEMA_VERSION: i32 = 8;

/// Database connection wrapper for Airlock state management.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open a database connection at the specified path.
    /// Creates the database file and initializes the schema if it doesn't exist.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| AirlockError::Database(format!("Failed to open database: {}", e)))?;

        let db = Self { conn };
        db.initialize()?;
        Ok(db)
    }

    /// Open an in-memory database (useful for testing).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| {
            AirlockError::Database(format!("Failed to open in-memory database: {}", e))
        })?;

        let db = Self { conn };
        db.initialize()?;
        Ok(db)
    }

    /// Open the default database using AirlockPaths.
    ///
    /// For the schema v4 migration (stage-based pipeline), this will delete
    /// the existing database if it's from a previous schema version.
    /// This is a one-time migration strategy that avoids complex data migration.
    pub fn open_default() -> Result<Self> {
        let paths = AirlockPaths::new()?;
        paths.ensure_dirs()?;
        let db_path = paths.database();

        // Check if we need to delete the old database (one-time migration for v4)
        Self::maybe_delete_old_database(&db_path)?;

        Self::open(&db_path)
    }

    /// Check schema version of existing database and delete if incompatible.
    ///
    /// For versions < 4 (pre-stage-based pipeline), we delete the database
    /// since the schema is incompatible. Future schema changes (v4+) should
    /// use proper migrations in the `migrate()` function.
    fn maybe_delete_old_database(path: &Path) -> Result<()> {
        if !path.exists() {
            tracing::debug!("No existing database at {:?}", path);
            return Ok(());
        }

        tracing::debug!("Checking existing database at {:?}", path);

        // Open temporarily to check version
        let conn = Connection::open(path)
            .map_err(|e| AirlockError::Database(format!("Failed to open database: {}", e)))?;

        // Check if schema_version table exists
        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schema_version'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| {
                AirlockError::Database(format!("Failed to check schema version table: {}", e))
            })?;

        if !table_exists {
            // Very old database without version tracking, delete it
            tracing::warn!("Database has no schema_version table, deleting for migration");
            drop(conn);
            Self::delete_database(path)?;
            return Ok(());
        }

        use rusqlite::OptionalExtension;
        let version: Option<i32> = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|e| AirlockError::Database(format!("Failed to get schema version: {}", e)))?;

        let current_version = version.unwrap_or(0);
        tracing::debug!("Existing database schema version: {}", current_version);

        // Must close connection before deleting the file
        drop(conn);

        // Delete database if schema is incompatible (pre-v4 stage-based pipeline)
        // Version 4 introduced breaking changes that require a fresh database
        if current_version < 4 {
            tracing::warn!(
                "Deleting old database (schema v{}) for stage-based pipeline migration",
                current_version
            );
            Self::delete_database(path)?;
        }

        Ok(())
    }

    /// Delete a database file.
    fn delete_database(path: &Path) -> Result<()> {
        std::fs::remove_file(path)
            .map_err(|e| AirlockError::Database(format!("Failed to delete old database: {}", e)))?;

        // Also try to delete WAL and SHM files if they exist
        let wal_path = path.with_extension("sqlite-wal");
        let shm_path = path.with_extension("sqlite-shm");
        let _ = std::fs::remove_file(wal_path);
        let _ = std::fs::remove_file(shm_path);

        tracing::info!("Deleted old database for schema migration");
        Ok(())
    }

    /// Initialize the database schema.
    fn initialize(&self) -> Result<()> {
        // Enable foreign keys
        self.conn
            .execute("PRAGMA foreign_keys = ON", [])
            .map_err(|e| AirlockError::Database(format!("Failed to enable foreign keys: {}", e)))?;

        // Check current schema version
        let current_version = self.get_schema_version()?;

        if current_version == 0 {
            // Fresh database, create all tables
            self.create_schema()?;
            self.set_schema_version(SCHEMA_VERSION)?;
            tracing::info!("Initialized database schema version {}", SCHEMA_VERSION);
        } else if current_version < SCHEMA_VERSION {
            // Run migrations
            self.migrate(current_version)?;
        }

        Ok(())
    }

    /// Get the current schema version.
    pub(crate) fn get_schema_version(&self) -> Result<i32> {
        // Check if schema_version table exists
        let table_exists: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='schema_version'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AirlockError::Database(format!("Failed to check schema version table: {}", e)))?;

        if !table_exists {
            return Ok(0);
        }

        use rusqlite::OptionalExtension;
        let version: Option<i32> = self
            .conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|e| AirlockError::Database(format!("Failed to get schema version: {}", e)))?;

        Ok(version.unwrap_or(0))
    }

    /// Set the schema version.
    fn set_schema_version(&self, version: i32) -> Result<()> {
        self.conn
            .execute("DELETE FROM schema_version", [])
            .map_err(|e| {
                AirlockError::Database(format!("Failed to clear schema version: {}", e))
            })?;

        self.conn
            .execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                [version],
            )
            .map_err(|e| AirlockError::Database(format!("Failed to set schema version: {}", e)))?;

        Ok(())
    }

    /// Create the initial database schema.
    fn create_schema(&self) -> Result<()> {
        let statements = [
            schema::CREATE_SCHEMA_VERSION_TABLE,
            schema::CREATE_REPOS_TABLE,
            schema::CREATE_RUNS_TABLE,
            schema::CREATE_JOB_RESULTS_TABLE,
            schema::CREATE_STEP_RESULTS_TABLE,
            schema::CREATE_SYNC_LOG_TABLE,
            schema::CREATE_RUNS_REPO_INDEX,
            schema::CREATE_JOB_RESULTS_RUN_INDEX,
            schema::CREATE_STEP_RESULTS_RUN_INDEX,
            schema::CREATE_STEP_RESULTS_JOB_INDEX,
            schema::CREATE_SYNC_LOG_REPO_INDEX,
        ];

        for stmt in statements {
            self.conn.execute(stmt, []).map_err(|e| {
                AirlockError::Database(format!("Failed to execute schema statement: {}", e))
            })?;
        }

        Ok(())
    }

    /// Run database migrations from the given version to the current version.
    ///
    /// Note: For the v4 migration (stage-based pipeline), we use a one-time
    /// database reset strategy instead of incremental migrations. This is
    /// handled in `maybe_delete_old_database()`.
    ///
    /// Future migrations (v4+) should be handled here with proper ALTER TABLE
    /// statements.
    fn migrate(&self, from_version: i32) -> Result<()> {
        tracing::info!(
            "Migrating database from version {} to {}",
            from_version,
            SCHEMA_VERSION
        );

        // Version < 4 requires database deletion (incompatible schema).
        // This should have been handled by maybe_delete_old_database(), but
        // if we get here somehow, fail with a clear error message.
        if from_version < 4 {
            return Err(AirlockError::Database(format!(
                "Cannot migrate from schema version {} to {}. \
                 Please delete ~/.airlock/state.sqlite and restart the daemon.",
                from_version, SCHEMA_VERSION
            )));
        }

        // Version 5: Add stage_order column to stage_results
        if from_version < 5 {
            tracing::info!("Migrating to schema version 5: adding stage_order column");
            self.conn
                .execute(schema::ADD_STAGE_ORDER_COLUMN, [])
                .map_err(|e| {
                    AirlockError::Database(format!("Failed to add stage_order column: {}", e))
                })?;
        }

        // Version 6: Remove artifacts_path column from stage_results
        if from_version < 6 {
            tracing::info!("Migrating to schema version 6: removing artifacts_path column");
            // SQLite doesn't support DROP COLUMN, so we recreate the table
            self.conn
                .execute(schema::MIGRATE_STAGE_RESULTS_V6, [])
                .map_err(|e| {
                    AirlockError::Database(format!(
                        "Failed to create new stage_results table: {}",
                        e
                    ))
                })?;
            self.conn
                .execute(schema::MIGRATE_STAGE_RESULTS_V6_COPY, [])
                .map_err(|e| {
                    AirlockError::Database(format!("Failed to copy stage_results data: {}", e))
                })?;
            self.conn
                .execute(schema::MIGRATE_STAGE_RESULTS_V6_DROP, [])
                .map_err(|e| {
                    AirlockError::Database(format!("Failed to drop old stage_results table: {}", e))
                })?;
            self.conn
                .execute(schema::MIGRATE_STAGE_RESULTS_V6_RENAME, [])
                .map_err(|e| {
                    AirlockError::Database(format!("Failed to rename stage_results table: {}", e))
                })?;
            // Recreate the index
            self.conn
                .execute(schema::CREATE_STAGE_RESULTS_RUN_INDEX, [])
                .map_err(|e| {
                    AirlockError::Database(format!("Failed to recreate stage_results index: {}", e))
                })?;
        }

        // Version 7: Add superseded column to runs
        if from_version < 7 {
            tracing::info!("Migrating to schema version 7: adding superseded column to runs");
            self.conn
                .execute(schema::ADD_SUPERSEDED_COLUMN, [])
                .map_err(|e| {
                    AirlockError::Database(format!("Failed to add superseded column: {}", e))
                })?;
        }

        // Version 8: Add workflow tracking to runs, add job_results, replace stage_results with step_results
        if from_version < 8 {
            tracing::info!(
                "Migrating to schema version 8: workflow tracking, job_results, step_results"
            );

            // Add workflow columns to runs
            self.conn
                .execute(schema::MIGRATE_RUNS_V8_ADD_WORKFLOW_FILE, [])
                .map_err(|e| {
                    AirlockError::Database(format!(
                        "Failed to add workflow_file column to runs: {}",
                        e
                    ))
                })?;
            self.conn
                .execute(schema::MIGRATE_RUNS_V8_ADD_WORKFLOW_NAME, [])
                .map_err(|e| {
                    AirlockError::Database(format!(
                        "Failed to add workflow_name column to runs: {}",
                        e
                    ))
                })?;

            // Create job_results table
            self.conn
                .execute(schema::MIGRATE_V8_CREATE_JOB_RESULTS, [])
                .map_err(|e| {
                    AirlockError::Database(format!("Failed to create job_results table: {}", e))
                })?;

            // Drop old stage_results and create new step_results
            self.conn
                .execute(schema::MIGRATE_V8_DROP_STAGE_RESULTS, [])
                .map_err(|e| {
                    AirlockError::Database(format!("Failed to drop stage_results table: {}", e))
                })?;
            self.conn
                .execute(schema::MIGRATE_V8_CREATE_STEP_RESULTS, [])
                .map_err(|e| {
                    AirlockError::Database(format!("Failed to create step_results table: {}", e))
                })?;

            // Create indexes
            self.conn
                .execute(schema::CREATE_JOB_RESULTS_RUN_INDEX, [])
                .map_err(|e| {
                    AirlockError::Database(format!("Failed to create job_results index: {}", e))
                })?;
            self.conn
                .execute(schema::CREATE_STEP_RESULTS_RUN_INDEX, [])
                .map_err(|e| {
                    AirlockError::Database(format!(
                        "Failed to create step_results run index: {}",
                        e
                    ))
                })?;
            self.conn
                .execute(schema::CREATE_STEP_RESULTS_JOB_INDEX, [])
                .map_err(|e| {
                    AirlockError::Database(format!(
                        "Failed to create step_results job index: {}",
                        e
                    ))
                })?;
        }

        self.set_schema_version(SCHEMA_VERSION)?;
        Ok(())
    }
}
