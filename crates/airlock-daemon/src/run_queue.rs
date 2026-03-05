//! Per-repo run serialization queue.
//!
//! Ensures only one pipeline run executes at a time for each repository.
//! When a new run arrives for a repo that already has an active run,
//! the active run is cancelled (via its `CancellationToken`) and the new
//! run waits for the semaphore permit to become available.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

/// A run tracked by the queue — either running or waiting for the permit.
struct TrackedRun {
    id: u64,
    token: CancellationToken,
    refs: Vec<String>,
}

/// Per-repository slot holding a semaphore (capacity 1) and tracked runs.
struct RepoSlot {
    /// Only one run at a time can hold the permit.
    semaphore: Arc<Semaphore>,
    /// Monotonically increasing ID for runs in this slot.
    next_id: u64,
    /// The run currently holding the semaphore permit.
    running: Option<TrackedRun>,
    /// Runs waiting for the semaphore permit, in arrival order.
    pending: Vec<TrackedRun>,
}

/// Serializes pipeline runs within each repository.
///
/// Multiple repos can run in parallel, but within a single repo only one
/// run executes at a time. Newer runs cancel older ones.
pub struct RunQueue {
    slots: Mutex<HashMap<String, RepoSlot>>,
}

/// Guard returned by [`RunQueue::acquire`].
///
/// Holds the semaphore permit (released on drop) and provides the
/// cancellation token that the pipeline should monitor.
pub struct RunPermit {
    /// The pipeline should check `token.is_cancelled()` periodically.
    pub token: CancellationToken,
    /// Dropping this releases the slot for the next run.
    _permit: OwnedSemaphorePermit,
}

impl Default for RunQueue {
    fn default() -> Self {
        Self {
            slots: Mutex::new(HashMap::new()),
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

    /// Cancel the active run for `repo_id` without acquiring a new slot.
    ///
    /// When `ref_names` is provided, only cancels if the active run has
    /// overlapping refs. When `None`, cancels unconditionally.
    ///
    /// Used when superseded runs need their tokens cancelled but no new
    /// pipeline run will be started (e.g., all refs were forwarded directly).
    pub async fn cancel_active(&self, repo_id: &str, ref_names: Option<&[String]>) {
        let mut slots = self.slots.lock().await;
        if let Some(slot) = slots.get_mut(repo_id) {
            // Cancel the running run if refs overlap (or unconditionally).
            let cancel_running = match (&slot.running, ref_names) {
                (Some(running), Some(refs)) => refs_overlap(&running.refs, refs),
                (Some(_), None) => true,
                (None, _) => false,
            };
            if cancel_running {
                if let Some(running) = slot.running.take() {
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

    /// Acquire the run slot for `repo_id`.
    ///
    /// `ref_names` are the refs this run will process (e.g.
    /// `["refs/heads/main"]`). If the active run has overlapping refs, it
    /// is cancelled so the new run supersedes it. If refs don't overlap,
    /// the new run queues behind without cancelling.
    ///
    /// Returns a [`RunPermit`] whose `token` the pipeline should monitor.
    pub async fn acquire(&self, repo_id: &str, ref_names: &[String]) -> RunPermit {
        let (semaphore, token, run_id) = {
            let mut slots = self.slots.lock().await;
            let slot = slots
                .entry(repo_id.to_string())
                .or_insert_with(|| RepoSlot {
                    semaphore: Arc::new(Semaphore::new(1)),
                    next_id: 0,
                    running: None,
                    pending: Vec::new(),
                });

            // Only cancel the running run if refs overlap (same branch pushed again).
            let cancel_running = match &slot.running {
                Some(r) => refs_overlap(&r.refs, ref_names),
                None => false,
            };
            if cancel_running {
                if let Some(running) = slot.running.take() {
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

        // Wait until the previous run releases its permit.
        let permit = semaphore
            .acquire_owned()
            .await
            .expect("semaphore should never be closed");

        // Promote this run from pending to running now that it holds the permit.
        {
            let mut slots = self.slots.lock().await;
            if let Some(slot) = slots.get_mut(repo_id) {
                slot.pending.retain(|p| p.id != run_id);
                slot.running = Some(TrackedRun {
                    id: run_id,
                    token: token.clone(),
                    refs: ref_names.to_vec(),
                });
            }
        }

        RunPermit {
            token,
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
            // Simulate a long-running pipeline that checks for cancellation
            loop {
                if permit.token.is_cancelled() {
                    break;
                }
                sleep(Duration::from_millis(5)).await;
            }
            // Token should be cancelled
            assert!(permit.token.is_cancelled());
        });

        // Give h1 time to acquire
        sleep(Duration::from_millis(10)).await;

        let q = queue.clone();
        let h2 = tokio::spawn(async move {
            // Same branch — should cancel h1's token
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

        // Acquire a slot and hold it
        let q = queue.clone();
        let h1 = tokio::spawn(async move {
            let permit = q.acquire("repo-1", &branch).await;
            // Wait for cancellation
            loop {
                if permit.token.is_cancelled() {
                    break;
                }
                sleep(Duration::from_millis(5)).await;
            }
            assert!(permit.token.is_cancelled());
        });

        // Give h1 time to acquire
        sleep(Duration::from_millis(10)).await;

        // cancel_active with None should cancel unconditionally
        queue.cancel_active("repo-1", None).await;

        h1.await.unwrap();

        // cancel_active on a non-existent repo should be a no-op
        queue.cancel_active("repo-nonexistent", None).await;
    }

    #[tokio::test]
    async fn test_queue_different_branches() {
        // When two runs target different branches of the same repo,
        // the second should queue (wait) without cancelling the first.
        let queue = Arc::new(RunQueue::new());
        let branch_a = refs(&["refs/heads/feature-a"]);
        let branch_b = refs(&["refs/heads/feature-b"]);

        let q = queue.clone();
        let h1 = tokio::spawn(async move {
            let permit = q.acquire("repo-1", &branch_a).await;
            // Should NOT be cancelled by branch-b's acquire
            sleep(Duration::from_millis(60)).await;
            assert!(
                !permit.token.is_cancelled(),
                "branch-a run should not be cancelled by branch-b"
            );
            drop(permit);
        });

        // Give h1 time to acquire
        sleep(Duration::from_millis(10)).await;

        let q = queue.clone();
        let h2 = tokio::spawn(async move {
            // Different branch — should NOT cancel h1, just wait
            let permit = q.acquire("repo-1", &branch_b).await;
            assert!(!permit.token.is_cancelled());
            drop(permit);
        });

        h1.await.unwrap();
        h2.await.unwrap();
    }

    #[tokio::test]
    async fn test_cancel_active_ref_aware() {
        let queue = Arc::new(RunQueue::new());
        let branch_main = refs(&["refs/heads/main"]);
        let branch_other = refs(&["refs/heads/other"]);

        // Acquire a slot running main
        let q = queue.clone();
        let h1 = tokio::spawn(async move {
            let permit = q.acquire("repo-1", &branch_main).await;
            // Wait a bit — should NOT be cancelled by a non-overlapping cancel
            sleep(Duration::from_millis(60)).await;
            assert!(
                !permit.token.is_cancelled(),
                "cancel_active with non-overlapping refs should not cancel"
            );
            drop(permit);
        });

        // Give h1 time to acquire
        sleep(Duration::from_millis(10)).await;

        // cancel_active with different refs should be a no-op
        queue.cancel_active("repo-1", Some(&branch_other)).await;

        h1.await.unwrap();
    }

    /// Regression: cancel_active must target the *running* run, not a
    /// queued run that overwrote the slot metadata.
    #[tokio::test]
    async fn test_cancel_active_with_queued_non_overlapping_run() {
        let queue = Arc::new(RunQueue::new());
        let branch_a = refs(&["refs/heads/feature-a"]);
        let branch_b = refs(&["refs/heads/feature-b"]);

        // Run A acquires the permit for branch-a.
        let q = queue.clone();
        let ba = branch_a.clone();
        let h1 = tokio::spawn(async move {
            let permit = q.acquire("repo-1", &ba).await;
            // Wait for cancellation
            loop {
                if permit.token.is_cancelled() {
                    break;
                }
                sleep(Duration::from_millis(1)).await;
            }
            assert!(permit.token.is_cancelled());
        });

        // Give h1 time to acquire the permit.
        sleep(Duration::from_millis(10)).await;

        // Run B queues behind A with non-overlapping branch-b.
        let q = queue.clone();
        let h2 = tokio::spawn(async move {
            let permit = q.acquire("repo-1", &branch_b).await;
            assert!(!permit.token.is_cancelled());
            drop(permit);
        });

        // Give h2 time to enter the pending queue.
        sleep(Duration::from_millis(10)).await;

        // cancel_active targeting branch-a must cancel the *running* run A,
        // not be confused by queued run B's metadata.
        queue.cancel_active("repo-1", Some(&branch_a)).await;

        h1.await.unwrap();
        h2.await.unwrap();
    }
}
