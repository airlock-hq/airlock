//! Sync logic for upstream synchronization.
//!
//! This module provides functionality for syncing the local gate repository
//! with the upstream remote. It includes:
//! - Stale check (>5 seconds since last sync)
//! - File-based locking to prevent concurrent syncs
//! - Upstream fetch with proper error handling

use airlock_core::{git, AirlockPaths, Database, Repo};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// Default stale threshold in seconds.
/// If the repo hasn't been synced in this time, it's considered stale.
pub const STALE_THRESHOLD_SECS: i64 = 5;

/// Maximum time to wait for a lock in milliseconds.
pub const LOCK_TIMEOUT_MS: u64 = 30000;

/// Result of a sync operation.
#[derive(Debug, Clone)]
pub struct SyncResult {
    /// Whether the sync was performed (false if not stale or skipped).
    pub synced: bool,
    /// Whether the sync succeeded (only meaningful if synced is true).
    pub success: bool,
    /// Error message if the sync failed.
    pub error: Option<String>,
    /// Timestamp when sync occurred (or when check was performed).
    pub timestamp: i64,
    /// Whether sync was skipped due to not being stale.
    pub skipped_not_stale: bool,
}

impl SyncResult {
    /// Create a result for a skipped sync (not stale).
    pub fn skipped_not_stale() -> Self {
        Self {
            synced: false,
            success: true,
            error: None,
            timestamp: now(),
            skipped_not_stale: true,
        }
    }

    /// Create a result for a successful sync.
    pub fn success() -> Self {
        Self {
            synced: true,
            success: true,
            error: None,
            timestamp: now(),
            skipped_not_stale: false,
        }
    }

    /// Create a result for a failed sync.
    pub fn failed(error: String) -> Self {
        Self {
            synced: true,
            success: false,
            error: Some(error),
            timestamp: now(),
            skipped_not_stale: false,
        }
    }
}

/// Get current Unix timestamp.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Check if a repo is stale (needs sync).
///
/// A repo is considered stale if:
/// - It has never been synced (last_sync is None)
/// - It was last synced more than STALE_THRESHOLD_SECS seconds ago
#[cfg(test)]
pub fn is_stale(repo: &Repo) -> bool {
    match repo.last_sync {
        None => true,
        Some(last_sync) => {
            let current = now();
            let elapsed = current - last_sync;
            elapsed > STALE_THRESHOLD_SECS
        }
    }
}

/// Check if a repo is stale with a custom threshold.
pub fn is_stale_with_threshold(repo: &Repo, threshold_secs: i64) -> bool {
    match repo.last_sync {
        None => true,
        Some(last_sync) => {
            let current = now();
            let elapsed = current - last_sync;
            elapsed > threshold_secs
        }
    }
}

/// A file-based lock for coordinating sync operations.
///
/// This provides a simple, cross-platform locking mechanism using
/// lock files. The lock is automatically released when dropped.
pub struct SyncLock {
    lock_path: std::path::PathBuf,
    _file: File,
}

impl SyncLock {
    /// Try to acquire a lock for the given repo.
    ///
    /// Returns `Ok(Some(lock))` if acquired, `Ok(None)` if already locked,
    /// or `Err` on I/O errors.
    pub fn try_acquire(paths: &AirlockPaths, repo_id: &str) -> io::Result<Option<Self>> {
        let lock_path = paths.repo_lock(repo_id);

        // Ensure locks directory exists
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Check if lock file exists and is recent
        if lock_path.exists() {
            // Check if the lock is stale (older than LOCK_TIMEOUT_MS)
            if let Ok(metadata) = fs::metadata(&lock_path) {
                if let Ok(modified) = metadata.modified() {
                    let age = SystemTime::now()
                        .duration_since(modified)
                        .unwrap_or(Duration::ZERO);

                    if age < Duration::from_millis(LOCK_TIMEOUT_MS) {
                        // Lock is still valid, another process has it
                        debug!("Lock file exists and is recent: {}", lock_path.display());
                        return Ok(None);
                    }

                    // Lock is stale, we can take over
                    debug!(
                        "Removing stale lock file (age: {:?}): {}",
                        age,
                        lock_path.display()
                    );
                }
            }
        }

        // Try to create/truncate the lock file
        let file = File::create(&lock_path)?;

        // Write our PID to the lock file for debugging
        let mut file = file;
        let _ = writeln!(file, "{}", std::process::id());

        debug!("Acquired sync lock: {}", lock_path.display());

        Ok(Some(Self {
            lock_path,
            _file: file,
        }))
    }

    /// Acquire a lock, waiting up to the timeout.
    ///
    /// Returns `Ok(lock)` if acquired, or `Err` if timeout or I/O error.
    pub async fn acquire_with_timeout(
        paths: &AirlockPaths,
        repo_id: &str,
        timeout_ms: u64,
    ) -> io::Result<Self> {
        let start = std::time::Instant::now();
        let timeout = Duration::from_millis(timeout_ms);
        let poll_interval = Duration::from_millis(100);

        loop {
            match Self::try_acquire(paths, repo_id)? {
                Some(lock) => return Ok(lock),
                None => {
                    if start.elapsed() >= timeout {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            format!("Timeout waiting for sync lock for repo {}", repo_id),
                        ));
                    }
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }
    }
}

impl Drop for SyncLock {
    fn drop(&mut self) {
        // Remove the lock file when we're done
        if let Err(e) = fs::remove_file(&self.lock_path) {
            warn!(
                "Failed to remove lock file {}: {}",
                self.lock_path.display(),
                e
            );
        } else {
            debug!("Released sync lock: {}", self.lock_path.display());
        }
    }
}

/// Get branch names that have active pipeline runs (un-forwarded commits).
/// Returns `None` on DB error — callers should treat all branches as protected
/// to avoid silently dropping un-forwarded commits.
pub fn get_protected_branches(db: &Database, repo_id: &str) -> Option<HashSet<String>> {
    match db.list_active_runs(repo_id) {
        Ok(runs) => Some(
            runs.into_iter()
                .map(|r| {
                    r.branch
                        .strip_prefix("refs/heads/")
                        .unwrap_or(&r.branch)
                        .to_string()
                })
                .filter(|b| !b.is_empty())
                .collect(),
        ),
        Err(e) => {
            warn!(
                "Failed to query active runs: {}, treating all branches as protected",
                e
            );
            None
        }
    }
}

/// List all local branches in a gate repo as a `HashSet`.
/// Used as a fallback when the DB is unreachable — treating every branch as
/// protected ensures we rebase (preserving commits) instead of force-updating.
pub fn all_local_branches(gate_path: &std::path::Path) -> HashSet<String> {
    git::list_local_branches(gate_path)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// Perform a sync operation for a repo if it's stale.
///
/// This is the main entry point for sync-on-fetch. It:
/// 1. Checks if the repo is stale
/// 2. Acquires a lock to prevent concurrent syncs
/// 3. Fetches from upstream
///
/// Note: This function does NOT update the database - that must be done
/// by the caller after a successful sync.
pub async fn sync_if_stale(
    paths: &AirlockPaths,
    repo: &Repo,
    protected_branches: &HashSet<String>,
) -> SyncResult {
    sync_if_stale_with_threshold(paths, repo, STALE_THRESHOLD_SECS, protected_branches).await
}

/// Perform a sync operation for a repo if it's stale, with a custom threshold.
///
/// Note: This function does NOT update the database - that must be done
/// by the caller after a successful sync.
pub async fn sync_if_stale_with_threshold(
    paths: &AirlockPaths,
    repo: &Repo,
    threshold_secs: i64,
    protected_branches: &HashSet<String>,
) -> SyncResult {
    // Check if stale
    if !is_stale_with_threshold(repo, threshold_secs) {
        debug!(
            "Repo {} is not stale (last_sync: {:?}), skipping sync",
            repo.id, repo.last_sync
        );
        return SyncResult::skipped_not_stale();
    }

    debug!("Repo {} is stale, attempting sync", repo.id);

    // Try to acquire lock
    let lock = match SyncLock::try_acquire(paths, &repo.id) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            // Another process is syncing, wait for it
            debug!(
                "Another process is syncing repo {}, waiting for lock",
                repo.id
            );
            match SyncLock::acquire_with_timeout(paths, &repo.id, LOCK_TIMEOUT_MS).await {
                Ok(lock) => lock,
                Err(e) => {
                    warn!("Failed to acquire sync lock for repo {}: {}", repo.id, e);
                    return SyncResult::failed(format!("Failed to acquire lock: {}", e));
                }
            }
        }
        Err(e) => {
            warn!("Failed to acquire sync lock for repo {}: {}", repo.id, e);
            return SyncResult::failed(format!("Failed to acquire lock: {}", e));
        }
    };

    // Perform the sync
    let result = do_sync(&repo.gate_path, &repo.id, paths, protected_branches);

    // Lock is automatically released when dropped
    drop(lock);

    result
}

/// Perform the actual sync operation using smart sync.
///
/// Smart sync preserves un-forwarded local commits on protected branches
/// (those with active pipelines) by rebasing them on top of upstream.
/// Unprotected diverged branches are force-updated to match remote.
fn do_sync(
    gate_path: &std::path::Path,
    repo_id: &str,
    paths: &AirlockPaths,
    protected_branches: &HashSet<String>,
) -> SyncResult {
    let sync_worktree_dir = paths.sync_worktree_dir(repo_id);
    // Use smart sync to preserve un-forwarded commits in the gate
    match git::smart_sync_from_remote(
        gate_path,
        "origin",
        Some(&sync_worktree_dir),
        git::ConflictResolver::Agent,
        protected_branches,
    ) {
        Ok(report) => {
            if report.warnings.is_empty() {
                info!("Successfully synced repo {} from origin", repo_id);
            } else {
                for warning in &report.warnings {
                    warn!("Sync warning for repo {}: {}", repo_id, warning);
                }
                info!(
                    "Synced repo {} from origin with {} warning(s)",
                    repo_id,
                    report.warnings.len()
                );
            }
            SyncResult::success()
        }
        Err(e) => {
            warn!("Failed to sync repo {} from origin: {}", repo_id, e);
            SyncResult::failed(e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_repo(id: &str, last_sync: Option<i64>) -> Repo {
        Repo {
            id: id.to_string(),
            working_path: PathBuf::from("/tmp/test-repo"),
            upstream_url: "git@github.com:user/repo.git".to_string(),
            gate_path: PathBuf::from("/tmp/.airlock/repos/test.git"),
            last_sync,
            created_at: now() - 3600,
        }
    }

    #[test]
    fn test_is_stale_never_synced() {
        let repo = create_test_repo("repo1", None);
        assert!(is_stale(&repo));
    }

    #[test]
    fn test_is_stale_recently_synced() {
        let repo = create_test_repo("repo1", Some(now() - 2)); // 2 seconds ago
        assert!(!is_stale(&repo));
    }

    #[test]
    fn test_is_stale_old_sync() {
        let repo = create_test_repo("repo1", Some(now() - 10)); // 10 seconds ago
        assert!(is_stale(&repo));
    }

    #[test]
    fn test_is_stale_at_threshold() {
        // At exactly 5 seconds, should not be stale (> not >=)
        let repo = create_test_repo("repo1", Some(now() - STALE_THRESHOLD_SECS));
        assert!(!is_stale(&repo));
    }

    #[test]
    fn test_is_stale_just_past_threshold() {
        let repo = create_test_repo("repo1", Some(now() - STALE_THRESHOLD_SECS - 1));
        assert!(is_stale(&repo));
    }

    #[test]
    fn test_is_stale_with_custom_threshold() {
        let repo = create_test_repo("repo1", Some(now() - 5)); // 5 seconds ago

        // With 10 second threshold, not stale
        assert!(!is_stale_with_threshold(&repo, 10));

        // With 3 second threshold, is stale
        assert!(is_stale_with_threshold(&repo, 3));
    }

    #[test]
    fn test_sync_result_skipped() {
        let result = SyncResult::skipped_not_stale();
        assert!(!result.synced);
        assert!(result.success);
        assert!(result.skipped_not_stale);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_sync_result_success() {
        let result = SyncResult::success();
        assert!(result.synced);
        assert!(result.success);
        assert!(!result.skipped_not_stale);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_sync_result_failed() {
        let result = SyncResult::failed("Connection error".to_string());
        assert!(result.synced);
        assert!(!result.success);
        assert!(!result.skipped_not_stale);
        assert_eq!(result.error, Some("Connection error".to_string()));
    }

    #[tokio::test]
    async fn test_sync_lock_acquire_and_release() {
        let temp = tempfile::TempDir::new().unwrap();
        let paths = AirlockPaths::with_root(temp.path().to_path_buf());
        paths.ensure_dirs().unwrap();

        // Acquire lock
        let lock = SyncLock::try_acquire(&paths, "repo1").unwrap();
        assert!(lock.is_some());
        let lock = lock.unwrap();

        // Try to acquire again - should fail
        let lock2 = SyncLock::try_acquire(&paths, "repo1").unwrap();
        assert!(lock2.is_none());

        // Release first lock
        drop(lock);

        // Now should succeed
        let lock3 = SyncLock::try_acquire(&paths, "repo1").unwrap();
        assert!(lock3.is_some());
    }
}
