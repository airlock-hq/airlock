//! SQL schema definitions for Airlock database.

/// Create repos table.
pub const CREATE_REPOS_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS repos (
        id TEXT PRIMARY KEY,
        working_path TEXT NOT NULL,
        upstream_url TEXT NOT NULL,
        gate_path TEXT NOT NULL,
        last_sync INTEGER,
        created_at INTEGER NOT NULL
    )
"#;

/// Runs table for workflow-based pipeline.
/// Note: Status is derived from job_results/step_results, not stored directly.
pub const CREATE_RUNS_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS runs (
        id TEXT PRIMARY KEY,
        repo_id TEXT NOT NULL,
        branch TEXT NOT NULL,
        base_sha TEXT NOT NULL,
        head_sha TEXT NOT NULL,
        current_stage TEXT,
        error TEXT,
        superseded INTEGER NOT NULL DEFAULT 0,
        workflow_file TEXT NOT NULL DEFAULT 'main.yml',
        workflow_name TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        FOREIGN KEY (repo_id) REFERENCES repos(id) ON DELETE CASCADE
    )
"#;

/// Job results table for tracking job execution within a run.
pub const CREATE_JOB_RESULTS_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS job_results (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        job_key TEXT NOT NULL,
        name TEXT,
        status TEXT NOT NULL DEFAULT 'pending',
        job_order INTEGER NOT NULL DEFAULT 0,
        started_at INTEGER,
        completed_at INTEGER,
        error TEXT,
        worktree_path TEXT,
        FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
    )
"#;

/// Step results table for tracking individual step execution within a job.
pub const CREATE_STEP_RESULTS_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS step_results (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        job_id TEXT NOT NULL,
        name TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'pending',
        step_order INTEGER NOT NULL DEFAULT 0,
        exit_code INTEGER,
        duration_ms INTEGER,
        error TEXT,
        started_at INTEGER,
        completed_at INTEGER,
        FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE,
        FOREIGN KEY (job_id) REFERENCES job_results(id) ON DELETE CASCADE
    )
"#;

/// Migration to add stage_order column (version 5).
pub const ADD_STAGE_ORDER_COLUMN: &str = r#"
    ALTER TABLE stage_results ADD COLUMN stage_order INTEGER NOT NULL DEFAULT 0
"#;

/// Migration to drop artifacts_path column (version 6).
/// SQLite doesn't support DROP COLUMN directly, so we recreate the table.
pub const MIGRATE_STAGE_RESULTS_V6: &str = r#"
    CREATE TABLE stage_results_new (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        name TEXT NOT NULL,
        status TEXT NOT NULL,
        stage_order INTEGER NOT NULL DEFAULT 0,
        exit_code INTEGER,
        duration_ms INTEGER,
        error TEXT,
        started_at INTEGER,
        completed_at INTEGER,
        FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
    )
"#;

pub const MIGRATE_STAGE_RESULTS_V6_COPY: &str = r#"
    INSERT INTO stage_results_new (id, run_id, name, status, stage_order, exit_code, duration_ms, error, started_at, completed_at)
    SELECT id, run_id, name, status, stage_order, exit_code, duration_ms, error, started_at, completed_at
    FROM stage_results
"#;

pub const MIGRATE_STAGE_RESULTS_V6_DROP: &str = r#"
    DROP TABLE stage_results
"#;

pub const MIGRATE_STAGE_RESULTS_V6_RENAME: &str = r#"
    ALTER TABLE stage_results_new RENAME TO stage_results
"#;

/// Migration v6: Recreate index on old stage_results table (used only during migration).
pub const CREATE_STAGE_RESULTS_RUN_INDEX: &str = r#"
    CREATE INDEX IF NOT EXISTS idx_stage_results_run_id ON stage_results(run_id)
"#;

/// Migration to add superseded column to runs (version 7).
pub const ADD_SUPERSEDED_COLUMN: &str = r#"
    ALTER TABLE runs ADD COLUMN superseded INTEGER NOT NULL DEFAULT 0
"#;

/// Sync log table for tracking repository synchronization.
pub const CREATE_SYNC_LOG_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS sync_log (
        id TEXT PRIMARY KEY,
        repo_id TEXT NOT NULL,
        success INTEGER NOT NULL,
        error TEXT,
        synced_at INTEGER NOT NULL,
        FOREIGN KEY (repo_id) REFERENCES repos(id) ON DELETE CASCADE
    )
"#;

/// Schema version table for migrations.
pub const CREATE_SCHEMA_VERSION_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS schema_version (
        version INTEGER PRIMARY KEY
    )
"#;

/// Index for faster lookups of runs by repo.
pub const CREATE_RUNS_REPO_INDEX: &str = r#"
    CREATE INDEX IF NOT EXISTS idx_runs_repo_id ON runs(repo_id)
"#;

/// Index for faster lookups of job results by run.
pub const CREATE_JOB_RESULTS_RUN_INDEX: &str = r#"
    CREATE INDEX IF NOT EXISTS idx_job_results_run_id ON job_results(run_id)
"#;

/// Index for faster lookups of step results by run.
pub const CREATE_STEP_RESULTS_RUN_INDEX: &str = r#"
    CREATE INDEX IF NOT EXISTS idx_step_results_run_id ON step_results(run_id)
"#;

/// Index for faster lookups of step results by job.
pub const CREATE_STEP_RESULTS_JOB_INDEX: &str = r#"
    CREATE INDEX IF NOT EXISTS idx_step_results_job_id ON step_results(job_id)
"#;

/// Index for faster lookups of sync logs by repo.
pub const CREATE_SYNC_LOG_REPO_INDEX: &str = r#"
    CREATE INDEX IF NOT EXISTS idx_sync_log_repo_id ON sync_log(repo_id)
"#;

// =============================================================================
// Migration v8: Add workflow tracking to runs, add job_results, replace
// stage_results with step_results
// =============================================================================

/// Migration v8: Add workflow_file and workflow_name columns to runs.
pub const MIGRATE_RUNS_V8_ADD_WORKFLOW_FILE: &str = r#"
    ALTER TABLE runs ADD COLUMN workflow_file TEXT NOT NULL DEFAULT 'main.yml'
"#;

pub const MIGRATE_RUNS_V8_ADD_WORKFLOW_NAME: &str = r#"
    ALTER TABLE runs ADD COLUMN workflow_name TEXT
"#;

/// Migration v8: Create job_results table.
pub const MIGRATE_V8_CREATE_JOB_RESULTS: &str = r#"
    CREATE TABLE IF NOT EXISTS job_results (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        job_key TEXT NOT NULL,
        name TEXT,
        status TEXT NOT NULL DEFAULT 'pending',
        job_order INTEGER NOT NULL DEFAULT 0,
        started_at INTEGER,
        completed_at INTEGER,
        error TEXT,
        FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
    )
"#;

/// Migration v8: Create new step_results table (replaces stage_results).
pub const MIGRATE_V8_CREATE_STEP_RESULTS: &str = r#"
    CREATE TABLE IF NOT EXISTS step_results (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        job_id TEXT NOT NULL,
        name TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'pending',
        step_order INTEGER NOT NULL DEFAULT 0,
        exit_code INTEGER,
        duration_ms INTEGER,
        error TEXT,
        started_at INTEGER,
        completed_at INTEGER,
        FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE,
        FOREIGN KEY (job_id) REFERENCES job_results(id) ON DELETE CASCADE
    )
"#;

/// Migration v8: Drop old stage_results table.
pub const MIGRATE_V8_DROP_STAGE_RESULTS: &str = r#"
    DROP TABLE IF EXISTS stage_results
"#;

/// Migration v9: Add worktree_path column to job_results for pool recovery.
pub const MIGRATE_V9_ADD_WORKTREE_PATH: &str = r#"
    ALTER TABLE job_results ADD COLUMN worktree_path TEXT
"#;
