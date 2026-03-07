//! Per-repo run queue with concurrent execution support.
//!
//! Allows up to `MAX_CONCURRENT_RUNS` pipeline runs to execute concurrently
//! for the same repository (e.g., different branches). When a new run arrives
//! for the same branch as an active run, the active run is cancelled (via its
//! `CancellationToken`) and the new run supersedes it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

/// Maximum number of concurrent runs per repository.
const MAX_CONCURRENT_RUNS: usize = 8;

/// A run tracked by the queue — either running or waiting for a permit.
struct TrackedRun {
    id: u64,
    token: CancellationToken,
    refs: Vec<String>,
}

/// Per-repository slot holding a semaphore and tracked runs.
struct RepoSlot {
    /// Up to MAX_CONCURRENT_RUNS permits available.
    semaphore: Arc<Semaphore>,
    /// Monotonically increasing ID for runs in this slot.
    next_id: u64,
    /// Runs currently holding semaphore permits.
    running: Vec<TrackedRun>,
    /// Runs waiting for a semaphore permit, in arrival order.
    pending: Vec<TrackedRun>,
}

type SlotMap = Arc<Mutex<HashMap<String, RepoSlot>>>;

/// Serializes and manages pipeline runs within each repository.
///
/// Multiple repos can run in parallel. Within a single repo, up to
/// `MAX_CONCURRENT_RUNS` runs can execute concurrently. Newer runs
/// targeting the same branch cancel older ones on that branch.
pub struct RunQueue {
    slots: SlotMap,
}

/// Guard returned by [`RunQueue::acquire`].
///
/// Holds the semaphore permit (released on drop) and provides the
/// cancellation token that the pipeline should monitor.
pub struct RunPermit {
    /// The pipeline should check `token.is_cancelled()` periodically.
    pub token: CancellationToken,
    /// Internal run ID for cleanup from the running vec.
    run_id: u64,
    /// Repo ID for cleanup.
    repo_id: String,
    /// Reference to the queue's slot map for cleanup on drop.
    slots: SlotMap,
    /// Dropping this releases the slot for the next run.
    _permit: OwnedSemaphorePermit,
}

impl Drop for RunPermit {
    fn drop(&mut self) {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(slot) = slots.get_mut(&self.repo_id) {
            slot.running.retain(|r| r.id != self.run_id);
        }
    }
}

impl Default for RunQueue {
    fn default() -> Self {
        Self {
            slots: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

/// Returns `true` if the two ref lists share at least one ref name.
fn refs_overlap(a: &[String], b: &[String]) -> bool {
    a.iter().any(|r| b.contains(r))
}

impl RunQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancel active runs for `repo_id` without acquiring a new slot.
    ///
    /// When `ref_names` is provided, only cancels runs with overlapping refs.
    /// When `None`, cancels all active and pending runs unconditionally.
    ///
    /// Used when superseded runs need their tokens cancelled but no new
    /// pipeline run will be started (e.g., all refs were forwarded directly).
    pub fn cancel_active(&self, repo_id: &str, ref_names: Option<&[String]>) {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(slot) = slots.get_mut(repo_id) {
            // Cancel running runs but keep them in the vec — the Drop impl
            // on RunPermit removes them once the permit is actually dropped.
            // Removing here would desync the vec from actual held permits.
            for running in &slot.running {
                let should_cancel = match ref_names {
                    Some(refs) => refs_overlap(&running.refs, refs),
                    None => true,
                };
                if should_cancel {
                    tracing::info!("Cancelling active run for repo {}", repo_id);
                    running.token.cancel();
                }
            }

            // Also cancel pending runs with overlapping refs.
            slot.pending.retain(|p| {
                let should_cancel = match ref_names {
                    Some(refs) => refs_overlap(&p.refs, refs),
                    None => true,
                };
                if should_cancel {
                    tracing::info!("Cancelling pending run for repo {}", repo_id);
                    p.token.cancel();
                    false
                } else {
                    true
                }
            });
        }
    }

    /// Acquire a run slot for `repo_id`.
    ///
    /// `ref_names` are the refs this run will process (e.g.
    /// `["refs/heads/main"]`). If any active run has overlapping refs, it
    /// is cancelled so the new run supersedes it. Runs with non-overlapping
    /// refs continue undisturbed.
    ///
    /// Returns a [`RunPermit`] whose `token` the pipeline should monitor.
    pub async fn acquire(&self, repo_id: &str, ref_names: &[String]) -> RunPermit {
        let (semaphore, token, run_id) = {
            let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
            let slot = slots
                .entry(repo_id.to_string())
                .or_insert_with(|| RepoSlot {
                    semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_RUNS)),
                    next_id: 0,
                    running: Vec::new(),
                    pending: Vec::new(),
                });

            // Cancel running runs with overlapping refs (same branch pushed again).
            // Keep them in the vec — the Drop impl on RunPermit removes them once
            // the permit is actually dropped. Removing here would free the semaphore
            // slot before the old run finishes, allowing two same-branch runs to
            // execute concurrently.
            for running in &slot.running {
                if refs_overlap(&running.refs, ref_names) {
                    tracing::info!(
                        "Cancelling active run for repo {} (overlapping refs) to make way for new run",
                        repo_id
                    );
                    running.token.cancel();
                }
            }

            // Cancel any pending runs with overlapping refs.
            slot.pending.retain(|p| {
                if refs_overlap(&p.refs, ref_names) {
                    tracing::info!(
                        "Cancelling pending run for repo {} (overlapping refs)",
                        repo_id
                    );
                    p.token.cancel();
                    false
                } else {
                    true
                }
            });

            // Install a fresh token for the incoming run in the pending list.
            let token = CancellationToken::new();
            let run_id = slot.next_id;
            slot.next_id += 1;
            slot.pending.push(TrackedRun {
                id: run_id,
                token: token.clone(),
                refs: ref_names.to_vec(),
            });

            (slot.semaphore.clone(), token, run_id)
        };

        // Wait until a permit is available.
        let permit = semaphore
            .acquire_owned()
            .await
            .expect("semaphore should never be closed");

        // Promote this run from pending to running now that it holds the permit.
        {
            let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(slot) = slots.get_mut(repo_id) {
                slot.pending.retain(|p| p.id != run_id);
                slot.running.push(TrackedRun {
                    id: run_id,
                    token: token.clone(),
                    refs: ref_names.to_vec(),
                });
            }
        }

        RunPermit {
            token,
            run_id,
            repo_id: repo_id.to_string(),
            slots: Arc::clone(&self.slots),
            _permit: permit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tokio::time::{sleep, Duration};

    fn refs(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[tokio::test]
    async fn test_sequential_same_repo() {
        let queue = Arc::new(RunQueue::new());
        let counter = Arc::new(AtomicU32::new(0));
        let branch = refs(&["refs/heads/main"]);

        // First run completes fully before second run starts.
        let q = queue.clone();
        let c = counter.clone();
        let b = branch.clone();
        {
            let permit = q.acquire("repo-1", &b).await;
            assert!(!permit.token.is_cancelled());
            c.fetch_add(1, Ordering::SeqCst);
            drop(permit);
        }

        let q = queue.clone();
        let c = counter.clone();
        {
            let permit = q.acquire("repo-1", &branch).await;
            // First run already completed
            assert_eq!(c.load(Ordering::SeqCst), 1);
            assert!(!permit.token.is_cancelled());
            drop(permit);
        }
    }

    #[tokio::test]
    async fn test_parallel_different_repos() {
        let queue = Arc::new(RunQueue::new());
        let running = Arc::new(AtomicU32::new(0));
        let branch = refs(&["refs/heads/main"]);

        let q = queue.clone();
        let r = running.clone();
        let b = branch.clone();
        let h1 = tokio::spawn(async move {
            let _permit = q.acquire("repo-1", &b).await;
            r.fetch_add(1, Ordering::SeqCst);
            sleep(Duration::from_millis(50)).await;
            r.fetch_sub(1, Ordering::SeqCst);
        });

        let q = queue.clone();
        let r = running.clone();
        let h2 = tokio::spawn(async move {
            let _permit = q.acquire("repo-2", &branch).await;
            r.fetch_add(1, Ordering::SeqCst);
            sleep(Duration::from_millis(50)).await;
            r.fetch_sub(1, Ordering::SeqCst);
        });

        // Wait a bit, then both should be running concurrently
        sleep(Duration::from_millis(20)).await;
        assert_eq!(running.load(Ordering::SeqCst), 2);

        h1.await.unwrap();
        h2.await.unwrap();
    }

    #[tokio::test]
    async fn test_cancel_active_run_same_branch() {
        let queue = Arc::new(RunQueue::new());
        let branch = refs(&["refs/heads/main"]);

        let q = queue.clone();
        let b = branch.clone();
        let h1 = tokio::spawn(async move {
            let permit = q.acquire("repo-1", &b).await;
            loop {
                if permit.token.is_cancelled() {
                    break;
                }
                sleep(Duration::from_millis(5)).await;
            }
            assert!(permit.token.is_cancelled());
        });

        sleep(Duration::from_millis(10)).await;

        let q = queue.clone();
        let h2 = tokio::spawn(async move {
            let permit = q.acquire("repo-1", &branch).await;
            assert!(!permit.token.is_cancelled());
            drop(permit);
        });

        h1.await.unwrap();
        h2.await.unwrap();
    }

    #[tokio::test]
    async fn test_cancel_active_without_acquire() {
        let queue = Arc::new(RunQueue::new());
        let branch = refs(&["refs/heads/main"]);

        let q = queue.clone();
        let h1 = tokio::spawn(async move {
            let permit = q.acquire("repo-1", &branch).await;
            loop {
                if permit.token.is_cancelled() {
                    break;
                }
                sleep(Duration::from_millis(5)).await;
            }
            assert!(permit.token.is_cancelled());
        });

        sleep(Duration::from_millis(10)).await;
        queue.cancel_active("repo-1", None);
        h1.await.unwrap();

        queue.cancel_active("repo-nonexistent", None);
    }

    #[tokio::test]
    async fn test_concurrent_different_branches_same_repo() {
        let queue = Arc::new(RunQueue::new());
        let running = Arc::new(AtomicU32::new(0));
        let branch_a = refs(&["refs/heads/feature-a"]);
        let branch_b = refs(&["refs/heads/feature-b"]);

        // Use barriers to synchronize: both tasks signal when they've acquired
        let (tx1, rx1) = tokio::sync::oneshot::channel::<()>();
        let (tx2, rx2) = tokio::sync::oneshot::channel::<()>();
        // Release channels to let tasks finish
        let (done_tx1, done_rx1) = tokio::sync::oneshot::channel::<()>();
        let (done_tx2, done_rx2) = tokio::sync::oneshot::channel::<()>();

        let q = queue.clone();
        let r = running.clone();
        let h1 = tokio::spawn(async move {
            let permit = q.acquire("repo-1", &branch_a).await;
            r.fetch_add(1, Ordering::SeqCst);
            let _ = tx1.send(());
            let _ = done_rx1.await;
            assert!(
                !permit.token.is_cancelled(),
                "branch-a run should not be cancelled by branch-b"
            );
            r.fetch_sub(1, Ordering::SeqCst);
            drop(permit);
        });

        let q = queue.clone();
        let r = running.clone();
        let h2 = tokio::spawn(async move {
            let permit = q.acquire("repo-1", &branch_b).await;
            r.fetch_add(1, Ordering::SeqCst);
            let _ = tx2.send(());
            let _ = done_rx2.await;
            assert!(!permit.token.is_cancelled());
            r.fetch_sub(1, Ordering::SeqCst);
            drop(permit);
        });

        // Wait for both to signal they're running
        let _ = rx1.await;
        let _ = rx2.await;
        assert_eq!(
            running.load(Ordering::SeqCst),
            2,
            "Both branches should run concurrently within same repo"
        );

        // Let them finish
        let _ = done_tx1.send(());
        let _ = done_tx2.send(());
        h1.await.unwrap();
        h2.await.unwrap();
    }

    #[tokio::test]
    async fn test_cancel_active_ref_aware() {
        let queue = Arc::new(RunQueue::new());
        let branch_main = refs(&["refs/heads/main"]);
        let branch_other = refs(&["refs/heads/other"]);

        let q = queue.clone();
        let h1 = tokio::spawn(async move {
            let permit = q.acquire("repo-1", &branch_main).await;
            sleep(Duration::from_millis(60)).await;
            assert!(
                !permit.token.is_cancelled(),
                "cancel_active with non-overlapping refs should not cancel"
            );
            drop(permit);
        });

        sleep(Duration::from_millis(10)).await;
        queue.cancel_active("repo-1", Some(&branch_other));
        h1.await.unwrap();
    }

    #[tokio::test]
    async fn test_cancel_active_with_queued_non_overlapping_run() {
        let queue = Arc::new(RunQueue::new());
        let branch_a = refs(&["refs/heads/feature-a"]);
        let branch_b = refs(&["refs/heads/feature-b"]);

        let q = queue.clone();
        let ba = branch_a.clone();
        let h1 = tokio::spawn(async move {
            let permit = q.acquire("repo-1", &ba).await;
            loop {
                if permit.token.is_cancelled() {
                    break;
                }
                sleep(Duration::from_millis(1)).await;
            }
            assert!(permit.token.is_cancelled());
        });

        sleep(Duration::from_millis(10)).await;

        let q = queue.clone();
        let h2 = tokio::spawn(async move {
            let permit = q.acquire("repo-1", &branch_b).await;
            assert!(!permit.token.is_cancelled());
            drop(permit);
        });

        sleep(Duration::from_millis(10)).await;
        queue.cancel_active("repo-1", Some(&branch_a));

        h1.await.unwrap();
        h2.await.unwrap();
    }

    #[tokio::test]
    async fn test_max_concurrent_reached_queues() {
        let queue = Arc::new(RunQueue::new());
        let acquired = Arc::new(AtomicU32::new(0));

        // Use a barrier: each task signals when acquired, waits for release
        let (release_tx, _) = tokio::sync::broadcast::channel::<()>(1);
        let mut ready_rxs = Vec::new();

        let mut handles = Vec::new();
        for i in 0..MAX_CONCURRENT_RUNS {
            let q = queue.clone();
            let a = acquired.clone();
            let branch = refs(&[&format!("refs/heads/branch-{}", i)]);
            let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
            ready_rxs.push(ready_rx);
            let mut release = release_tx.subscribe();
            handles.push(tokio::spawn(async move {
                let _permit = q.acquire("repo-1", &branch).await;
                a.fetch_add(1, Ordering::SeqCst);
                let _ = ready_tx.send(());
                let _ = release.recv().await;
                a.fetch_sub(1, Ordering::SeqCst);
            }));
        }

        // Wait for all MAX_CONCURRENT_RUNS tasks to acquire
        for rx in ready_rxs {
            let _ = rx.await;
        }
        assert_eq!(acquired.load(Ordering::SeqCst), MAX_CONCURRENT_RUNS as u32);

        // Try to acquire one more — should block since all permits are held
        let q = queue.clone();
        let a = acquired.clone();
        let extra_branch = refs(&["refs/heads/extra"]);
        let extra = tokio::spawn(async move {
            let _permit = q.acquire("repo-1", &extra_branch).await;
            a.fetch_add(1, Ordering::SeqCst);
            a.fetch_sub(1, Ordering::SeqCst);
        });

        // Give the extra task a moment to try acquiring (it should be blocked)
        sleep(Duration::from_millis(5)).await;
        assert_eq!(
            acquired.load(Ordering::SeqCst),
            MAX_CONCURRENT_RUNS as u32,
            "Extra run should be blocked"
        );

        // Release all held permits
        let _ = release_tx.send(());
        for h in handles {
            h.await.unwrap();
        }
        extra.await.unwrap();
    }
}
