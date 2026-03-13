//! Per-repo pool of reusable worktrees.
//!
//! Instead of a single persistent worktree per repo, this module manages a pool
//! of reusable worktree slots. Each job acquires a slot (reset to the target
//! commit with build caches preserved), and releases it back when done.
//!
//! If the pool is empty, a new slot is created. This allows concurrent pipeline
//! runs for the same repo without blocking each other.

use airlock_core::{AirlockPaths, Database};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// A lease on a pool worktree slot. The caller must explicitly release it
/// back to the pool when done (or keep it for paused jobs).
#[derive(Debug, Clone)]
pub struct PoolLease {
    /// Filesystem path to the worktree.
    pub path: PathBuf,
    /// Index of the slot in the pool.
    pub slot_index: usize,
}

/// Internal state for a single worktree slot.
struct PoolSlot {
    index: usize,
    path: PathBuf,
    in_use: bool,
}

/// Per-repo pool state.
struct RepoPool {
    slots: Vec<PoolSlot>,
    /// Next index to use when creating a new slot.
    next_index: usize,
}

impl RepoPool {
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            next_index: 0,
        }
    }

    /// Find an idle slot, mark it in-use, and return its info.
    fn acquire_idle(&mut self) -> Option<(usize, PathBuf)> {
        for slot in &mut self.slots {
            if !slot.in_use {
                slot.in_use = true;
                return Some((slot.index, slot.path.clone()));
            }
        }
        None
    }

    /// Allocate a new slot (in-use), returning (index, path). Does NOT create the
    /// worktree on disk — the caller must do that after releasing the lock.
    fn allocate_new(&mut self, repo_id: &str, paths: &AirlockPaths) -> (usize, PathBuf) {
        let index = self.next_index;
        self.next_index += 1;
        let path = paths.pool_worktree(repo_id, index);
        self.slots.push(PoolSlot {
            index,
            path: path.clone(),
            in_use: true,
        });
        (index, path)
    }

    /// Mark a slot as idle.
    fn release(&mut self, slot_index: usize) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.index == slot_index) {
            slot.in_use = false;
        }
    }
}

/// Thread-safe pool of reusable worktrees, organized per repository.
pub struct WorktreePool {
    inner: Mutex<HashMap<String, RepoPool>>,
}

impl Default for WorktreePool {
    fn default() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl WorktreePool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Acquire a worktree from the pool for the given repo.
    ///
    /// If an idle slot exists, it is reused (and reset to `head_sha`).
    /// Otherwise, a new slot is created.
    ///
    /// The worktree is reset via `reset_persistent_worktree` which preserves
    /// gitignored build caches.
    pub async fn acquire(
        &self,
        repo_id: &str,
        gate_path: &std::path::Path,
        head_sha: &str,
        paths: &AirlockPaths,
    ) -> Result<PoolLease, String> {
        let (slot_index, worktree_path, is_new) = {
            let mut pools = self.inner.lock().await;
            let pool = pools
                .entry(repo_id.to_string())
                .or_insert_with(RepoPool::new);

            if let Some((index, path)) = pool.acquire_idle() {
                (index, path, false)
            } else {
                let (index, path) = pool.allocate_new(repo_id, paths);
                (index, path, true)
            }
        };
        // Lock released — do blocking I/O outside the lock

        let gate = gate_path.to_path_buf();
        let wt = worktree_path.clone();
        let sha = head_sha.to_string();

        let result = tokio::task::spawn_blocking(move || {
            if is_new && !wt.exists() {
                debug!("Creating new pool worktree at {:?}", wt);
            }
            // reset_persistent_worktree creates if missing, resets if exists
            airlock_core::reset_persistent_worktree(&gate, &wt, &sha)
        })
        .await
        .map_err(|e| format!("Worktree reset task panicked: {e}"))?;

        if let Err(e) = result {
            // Release the slot back since we failed
            let mut pools = self.inner.lock().await;
            if let Some(pool) = pools.get_mut(repo_id) {
                pool.release(slot_index);
            }
            return Err(format!("Failed to reset pool worktree: {e}"));
        }

        debug!(
            "Acquired pool worktree slot {} for repo {} at {:?}",
            slot_index, repo_id, worktree_path
        );

        Ok(PoolLease {
            path: worktree_path,
            slot_index,
        })
    }

    /// Release a worktree back to the pool, making it available for reuse.
    pub async fn release(&self, repo_id: &str, slot_index: usize) {
        let mut pools = self.inner.lock().await;
        if let Some(pool) = pools.get_mut(repo_id) {
            pool.release(slot_index);
            debug!(
                "Released pool worktree slot {} for repo {}",
                slot_index, repo_id
            );
        }
    }

    /// Initialize the pool from existing worktrees on disk.
    ///
    /// Called at daemon startup to recover pool state after a crash/restart:
    /// 1. Scans `~/.airlock/worktrees/{repo_id}/pool-*` directories
    /// 2. Validates each via `is_valid_worktree()` — removes invalid ones
    /// 3. Registers valid worktrees as idle slots
    /// 4. Queries DB for `AwaitingApproval` jobs → marks their slots as in-use
    /// 5. Migrates legacy `persistent` dir to `pool-0` on first encounter
    pub async fn init_from_disk(&self, paths: &AirlockPaths, db: &Database) -> Result<(), String> {
        let worktrees_dir = paths.worktrees_dir();
        if !worktrees_dir.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(&worktrees_dir)
            .map_err(|e| format!("Failed to read worktrees dir: {e}"))?;

        let mut pools = self.inner.lock().await;

        for entry in entries.flatten() {
            let repo_dir = entry.path();
            if !repo_dir.is_dir() {
                continue;
            }

            let repo_id = match entry.file_name().into_string() {
                Ok(name) => name,
                Err(_) => continue,
            };

            let pool = pools.entry(repo_id.clone()).or_insert_with(RepoPool::new);

            // Remove legacy `persistent` worktree if it exists.
            // fs::rename breaks git worktree back-references, so we remove it
            // and let acquire() create a fresh pool-0 when needed.
            let persistent_path = repo_dir.join("persistent");
            if persistent_path.exists() {
                let gate_path = paths.repo_gate(&repo_id);
                info!(
                    "Removing legacy persistent worktree for repo {} (pool will recreate on demand)",
                    repo_id
                );
                if let Err(e) = airlock_core::remove_worktree(&gate_path, &persistent_path) {
                    warn!(
                        "Failed to remove legacy persistent worktree for repo {}: {}",
                        repo_id, e
                    );
                    // Continue — don't block init on migration failure
                }
            }

            // Scan for pool-* directories
            let repo_entries = match std::fs::read_dir(&repo_dir) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for wt_entry in repo_entries.flatten() {
                let name = match wt_entry.file_name().into_string() {
                    Ok(n) => n,
                    Err(_) => continue,
                };

                if !name.starts_with("pool-") {
                    continue;
                }

                let index: usize = match name.strip_prefix("pool-").and_then(|s| s.parse().ok()) {
                    Some(i) => i,
                    None => continue,
                };

                let wt_path = wt_entry.path();

                // Validate the worktree
                if !airlock_core::is_valid_worktree(&wt_path) {
                    warn!(
                        "Removing invalid pool worktree {:?} for repo {}",
                        wt_path, repo_id
                    );
                    // Find the gate path to prune
                    let gate_path = paths.repo_gate(&repo_id);
                    let _ = airlock_core::remove_worktree(&gate_path, &wt_path);
                    continue;
                }

                // Register as idle slot
                pool.slots.push(PoolSlot {
                    index,
                    path: wt_path,
                    in_use: false,
                });

                // Update next_index to be beyond all discovered indices
                if index >= pool.next_index {
                    pool.next_index = index + 1;
                }
            }
        }

        // Mark worktrees for AwaitingApproval jobs as in-use
        if let Ok(paused_jobs) = db.get_awaiting_approval_jobs_with_worktrees() {
            for (repo_id, job_key, wt_path) in &paused_jobs {
                if let Some(pool) = pools.get_mut(repo_id) {
                    let wt = PathBuf::from(wt_path);
                    for slot in &mut pool.slots {
                        if slot.path == wt {
                            slot.in_use = true;
                            debug!(
                                "Marked pool slot {} as in-use for paused job {} in repo {}",
                                slot.index, job_key, repo_id
                            );
                        }
                    }
                }
            }
        }

        // Log summary
        for (repo_id, pool) in pools.iter() {
            let idle = pool.slots.iter().filter(|s| !s.in_use).count();
            let in_use = pool.slots.iter().filter(|s| s.in_use).count();
            if !pool.slots.is_empty() {
                info!(
                    "Pool for repo {}: {} slots ({} idle, {} in-use)",
                    repo_id,
                    pool.slots.len(),
                    idle,
                    in_use
                );
            }
        }

        Ok(())
    }

    /// Find a lease for a worktree path that's already in-use (e.g., paused job's worktree).
    /// Returns None if the path is not tracked by the pool.
    pub async fn find_lease_by_path(
        &self,
        repo_id: &str,
        worktree_path: &std::path::Path,
    ) -> Option<PoolLease> {
        let pools = self.inner.lock().await;
        if let Some(pool) = pools.get(repo_id) {
            for slot in &pool.slots {
                if slot.path == worktree_path {
                    return Some(PoolLease {
                        path: slot.path.clone(),
                        slot_index: slot.index,
                    });
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use airlock_core::AirlockPaths;
    use std::path::Path;
    use tempfile::TempDir;

    /// Create a minimal bare git repo at `path` with an initial commit.
    fn create_bare_repo(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        let output = std::process::Command::new("git")
            .args(["init", "--bare"])
            .current_dir(path)
            .output()
            .unwrap();
        assert!(output.status.success(), "git init --bare failed");

        // Create an initial commit via a temp worktree
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("work");
        let output = std::process::Command::new("git")
            .args(["clone", &path.to_string_lossy(), &wt.to_string_lossy()])
            .output()
            .unwrap();
        assert!(output.status.success(), "git clone failed");

        std::fs::write(wt.join("README.md"), "init").unwrap();

        for (args, msg) in [
            (vec!["add", "."], "git add"),
            (vec!["config", "user.email", "test@test.com"], "git config"),
            (vec!["config", "user.name", "Test"], "git config"),
            (vec!["commit", "-m", "init"], "git commit"),
            (vec!["push", "origin", "HEAD"], "git push"),
        ] {
            let out = std::process::Command::new("git")
                .args(&args)
                .current_dir(&wt)
                .output()
                .unwrap();
            assert!(out.status.success(), "{} failed: {:?}", msg, out);
        }
    }

    /// Get HEAD sha from a bare repo.
    fn get_head_sha(gate_path: &Path) -> String {
        let output = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(gate_path)
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn setup() -> (TempDir, AirlockPaths, PathBuf, String) {
        let tmp = TempDir::new().unwrap();
        let paths = AirlockPaths::with_root(tmp.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let gate_path = paths.repo_gate("test-repo");
        create_bare_repo(&gate_path);
        let head_sha = get_head_sha(&gate_path);
        (tmp, paths, gate_path, head_sha)
    }

    #[tokio::test]
    async fn test_acquire_creates_new_slot_when_pool_empty() {
        let (_tmp, paths, gate_path, head_sha) = setup();
        let pool = WorktreePool::new();

        let lease = pool
            .acquire("test-repo", &gate_path, &head_sha, &paths)
            .await
            .unwrap();

        assert_eq!(lease.slot_index, 0);
        assert!(lease.path.exists());
        assert!(lease.path.to_string_lossy().contains("pool-0"));
    }

    #[tokio::test]
    async fn test_acquire_reuses_idle_slot() {
        let (_tmp, paths, gate_path, head_sha) = setup();
        let pool = WorktreePool::new();

        let lease1 = pool
            .acquire("test-repo", &gate_path, &head_sha, &paths)
            .await
            .unwrap();
        let idx = lease1.slot_index;
        let path = lease1.path.clone();

        pool.release("test-repo", idx).await;

        let lease2 = pool
            .acquire("test-repo", &gate_path, &head_sha, &paths)
            .await
            .unwrap();
        assert_eq!(lease2.slot_index, idx);
        assert_eq!(lease2.path, path);
    }

    #[tokio::test]
    async fn test_release_makes_slot_idle() {
        let (_tmp, paths, gate_path, head_sha) = setup();
        let pool = WorktreePool::new();

        let lease = pool
            .acquire("test-repo", &gate_path, &head_sha, &paths)
            .await
            .unwrap();

        // Before release, acquiring again should create a new slot
        let lease2 = pool
            .acquire("test-repo", &gate_path, &head_sha, &paths)
            .await
            .unwrap();
        assert_ne!(lease.slot_index, lease2.slot_index);

        // Release both
        pool.release("test-repo", lease.slot_index).await;
        pool.release("test-repo", lease2.slot_index).await;

        // Now acquiring should reuse one of the existing slots
        let lease3 = pool
            .acquire("test-repo", &gate_path, &head_sha, &paths)
            .await
            .unwrap();
        assert!(lease3.slot_index == lease.slot_index || lease3.slot_index == lease2.slot_index);
    }

    #[tokio::test]
    async fn test_concurrent_acquires_get_different_slots() {
        let (_tmp, paths, gate_path, head_sha) = setup();
        let pool = WorktreePool::new();

        let lease1 = pool
            .acquire("test-repo", &gate_path, &head_sha, &paths)
            .await
            .unwrap();
        let lease2 = pool
            .acquire("test-repo", &gate_path, &head_sha, &paths)
            .await
            .unwrap();

        assert_ne!(lease1.slot_index, lease2.slot_index);
        assert_ne!(lease1.path, lease2.path);
        assert!(lease1.path.exists());
        assert!(lease2.path.exists());
    }

    #[tokio::test]
    async fn test_init_from_disk_discovers_existing_worktrees() {
        let (_tmp, paths, gate_path, head_sha) = setup();

        // Pre-create pool worktrees on disk
        let wt0 = paths.pool_worktree("test-repo", 0);
        let wt1 = paths.pool_worktree("test-repo", 1);
        airlock_core::reset_persistent_worktree(&gate_path, &wt0, &head_sha).unwrap();
        airlock_core::reset_persistent_worktree(&gate_path, &wt1, &head_sha).unwrap();

        let pool = WorktreePool::new();
        let db = Database::open_in_memory().unwrap();
        pool.init_from_disk(&paths, &db).await.unwrap();

        // Both should be discovered as idle — acquiring should reuse them
        let lease = pool
            .acquire("test-repo", &gate_path, &head_sha, &paths)
            .await
            .unwrap();
        assert!(lease.slot_index <= 1);
    }

    #[tokio::test]
    async fn test_init_from_disk_skips_invalid_worktrees() {
        let (_tmp, paths, gate_path, head_sha) = setup();

        // Create a valid pool worktree
        let wt0 = paths.pool_worktree("test-repo", 0);
        airlock_core::reset_persistent_worktree(&gate_path, &wt0, &head_sha).unwrap();

        // Create an invalid pool worktree (just an empty dir)
        let wt1 = paths.pool_worktree("test-repo", 1);
        std::fs::create_dir_all(&wt1).unwrap();

        let pool = WorktreePool::new();
        let db = Database::open_in_memory().unwrap();
        pool.init_from_disk(&paths, &db).await.unwrap();

        // Only pool-0 should be discovered; pool-1 should have been removed
        let lease = pool
            .acquire("test-repo", &gate_path, &head_sha, &paths)
            .await
            .unwrap();
        assert_eq!(lease.slot_index, 0);
    }

    #[tokio::test]
    async fn test_legacy_persistent_migration() {
        let (_tmp, paths, gate_path, head_sha) = setup();

        // Create a legacy persistent worktree
        let persistent_path = paths.repo_worktree("test-repo");
        airlock_core::reset_persistent_worktree(&gate_path, &persistent_path, &head_sha).unwrap();
        assert!(persistent_path.exists());

        let pool = WorktreePool::new();
        let db = Database::open_in_memory().unwrap();
        pool.init_from_disk(&paths, &db).await.unwrap();

        // Persistent should be removed (not renamed — rename breaks git back-refs)
        assert!(!persistent_path.exists());

        // pool-0 is created fresh on first acquire, not by migration
        let lease = pool
            .acquire("test-repo", &gate_path, &head_sha, &paths)
            .await
            .unwrap();
        assert_eq!(lease.slot_index, 0);
        assert!(lease.path.exists());
    }

    #[tokio::test]
    async fn test_different_repos_independent() {
        let tmp = TempDir::new().unwrap();
        let paths = AirlockPaths::with_root(tmp.path().to_path_buf());
        paths.ensure_dirs().unwrap();

        let gate_a = paths.repo_gate("repo-a");
        create_bare_repo(&gate_a);
        let sha_a = get_head_sha(&gate_a);

        let gate_b = paths.repo_gate("repo-b");
        create_bare_repo(&gate_b);
        let sha_b = get_head_sha(&gate_b);

        let pool = WorktreePool::new();

        let lease_a = pool
            .acquire("repo-a", &gate_a, &sha_a, &paths)
            .await
            .unwrap();
        let lease_b = pool
            .acquire("repo-b", &gate_b, &sha_b, &paths)
            .await
            .unwrap();

        // Both should get slot 0 (independent pools)
        assert_eq!(lease_a.slot_index, 0);
        assert_eq!(lease_b.slot_index, 0);
        assert_ne!(lease_a.path, lease_b.path);
    }
}
