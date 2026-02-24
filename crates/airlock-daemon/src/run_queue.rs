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

/// Per-repository slot holding a semaphore (capacity 1) and the active
/// run's cancellation token.
struct RepoSlot {
    /// Only one run at a time can hold the permit.
    semaphore: Arc<Semaphore>,
    /// Token for the currently active run. Cancelled when a new run arrives.
    active_token: Option<CancellationToken>,
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

impl RunQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Acquire the run slot for `repo_id`.
    ///
    /// If another run is active for this repo, its cancellation token is
    /// cancelled first (signalling it to stop), then we wait for its
    /// semaphore permit to be released.
    ///
    /// Returns a [`RunPermit`] whose `token` the pipeline should monitor.
    pub async fn acquire(&self, repo_id: &str) -> RunPermit {
        let (semaphore, token) = {
            let mut slots = self.slots.lock().await;
            let slot = slots
                .entry(repo_id.to_string())
                .or_insert_with(|| RepoSlot {
                    semaphore: Arc::new(Semaphore::new(1)),
                    active_token: None,
                });

            // Cancel the currently active run (if any) so it stops quickly.
            if let Some(prev) = slot.active_token.take() {
                tracing::info!(
                    "Cancelling active run for repo {} to make way for new run",
                    repo_id
                );
                prev.cancel();
            }

            // Install a fresh token for the incoming run.
            let token = CancellationToken::new();
            slot.active_token = Some(token.clone());

            (slot.semaphore.clone(), token)
        };

        // Wait until the previous run releases its permit.
        let permit = semaphore
            .acquire_owned()
            .await
            .expect("semaphore should never be closed");

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

    #[tokio::test]
    async fn test_sequential_same_repo() {
        let queue = Arc::new(RunQueue::new());
        let counter = Arc::new(AtomicU32::new(0));

        // First run completes fully before second run starts.
        let q = queue.clone();
        let c = counter.clone();
        {
            let permit = q.acquire("repo-1").await;
            assert!(!permit.token.is_cancelled());
            c.fetch_add(1, Ordering::SeqCst);
            drop(permit);
        }

        let q = queue.clone();
        let c = counter.clone();
        {
            let permit = q.acquire("repo-1").await;
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

        let q = queue.clone();
        let r = running.clone();
        let h1 = tokio::spawn(async move {
            let _permit = q.acquire("repo-1").await;
            r.fetch_add(1, Ordering::SeqCst);
            sleep(Duration::from_millis(50)).await;
            r.fetch_sub(1, Ordering::SeqCst);
        });

        let q = queue.clone();
        let r = running.clone();
        let h2 = tokio::spawn(async move {
            let _permit = q.acquire("repo-2").await;
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
    async fn test_cancel_active_run() {
        let queue = Arc::new(RunQueue::new());

        let q = queue.clone();
        let h1 = tokio::spawn(async move {
            let permit = q.acquire("repo-1").await;
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
            // This should cancel h1's token
            let permit = q.acquire("repo-1").await;
            assert!(!permit.token.is_cancelled());
            drop(permit);
        });

        h1.await.unwrap();
        h2.await.unwrap();
    }
}
