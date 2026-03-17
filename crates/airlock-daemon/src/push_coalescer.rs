//! Push coalescing and deduplication.
//!
//! This module handles rapid pushes by:
//! 1. Debouncing - waiting a short period to collect multiple pushes
//! 2. Coalescing - merging ref updates from rapid consecutive pushes
//! 3. Superseding - marking old pending runs as superseded when new pushes arrive

use airlock_core::{Database, RefUpdate, Run};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;
use tracing::{debug, info};

/// How long to wait before processing a push (to allow coalescing).
const DEBOUNCE_DELAY: Duration = Duration::from_secs(2);

/// A pending push that's waiting to be processed.
#[derive(Debug, Clone)]
struct PendingPush {
    /// Accumulated ref updates from all pushes in this window.
    ref_updates: Vec<RefUpdate>,
    /// When the first push in this window was received.
    first_received: Instant,
    /// When the last push in this window was received.
    last_received: Instant,
}

/// Manages push coalescing for all repositories.
pub struct PushCoalescer {
    /// Pending pushes by repo_id.
    pending: Mutex<HashMap<String, PendingPush>>,
}

impl PushCoalescer {
    /// Create a new push coalescer.
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Record a push received for a repository.
    ///
    /// Returns `Some(ref_updates)` if the push should be processed now
    /// (either because debounce period has passed or this is a force flush).
    /// Returns `None` if the push was recorded for later processing.
    pub async fn record_push(
        &self,
        repo_id: &str,
        ref_updates: Vec<RefUpdate>,
    ) -> Option<Vec<RefUpdate>> {
        let mut pending = self.pending.lock().await;
        let now = Instant::now();

        if let Some(existing) = pending.get_mut(repo_id) {
            // Merge ref updates - newer updates for the same ref supersede older ones
            merge_ref_updates(&mut existing.ref_updates, ref_updates);
            existing.last_received = now;

            // Check if we should flush (debounce period passed since first push)
            if now.duration_since(existing.first_received) >= DEBOUNCE_DELAY {
                let updates = existing.ref_updates.clone();
                pending.remove(repo_id);
                debug!(
                    "Flushing coalesced push for repo {} ({} refs)",
                    repo_id,
                    updates.len()
                );
                return Some(updates);
            }

            debug!(
                "Coalesced push for repo {} (now {} refs)",
                repo_id,
                existing.ref_updates.len()
            );
            None
        } else {
            // First push in a new window
            pending.insert(
                repo_id.to_string(),
                PendingPush {
                    ref_updates,
                    first_received: now,
                    last_received: now,
                },
            );
            debug!("Recorded first push for repo {}", repo_id);
            None
        }
    }

    /// Check if any pending pushes are ready to be processed.
    ///
    /// Returns a list of (repo_id, ref_updates) for pushes that have passed
    /// the debounce period.
    pub async fn ready_pushes(&self) -> Vec<(String, Vec<RefUpdate>)> {
        let mut pending = self.pending.lock().await;
        let now = Instant::now();

        let mut ready = Vec::new();
        let mut to_remove = Vec::new();

        for (repo_id, push) in pending.iter() {
            if now.duration_since(push.first_received) >= DEBOUNCE_DELAY {
                ready.push((repo_id.clone(), push.ref_updates.clone()));
                to_remove.push(repo_id.clone());
            }
        }

        for repo_id in to_remove {
            pending.remove(&repo_id);
        }

        ready
    }
}

impl Default for PushCoalescer {
    fn default() -> Self {
        Self::new()
    }
}

/// Merge new ref updates into existing ones.
///
/// For each ref, the newer update (from `new`) supersedes the older one.
fn merge_ref_updates(existing: &mut Vec<RefUpdate>, new: Vec<RefUpdate>) {
    for new_update in new {
        // Find and replace existing update for the same ref, or append
        if let Some(existing_update) = existing
            .iter_mut()
            .find(|u| u.ref_name == new_update.ref_name)
        {
            // Keep the original old_sha but use the new new_sha
            existing_update.new_sha = new_update.new_sha;
        } else {
            existing.push(new_update);
        }
    }
}

/// Check for overlapping refs between a new push and existing active runs.
///
/// Returns true if any ref in the new push overlaps with an active run.
#[cfg(test)]
pub fn has_overlapping_refs(new_refs: &[RefUpdate], active_runs: &[Run]) -> bool {
    for run in active_runs {
        for new_ref in new_refs {
            if run
                .ref_updates
                .iter()
                .any(|r| r.ref_name == new_ref.ref_name)
            {
                return true;
            }
        }
    }
    false
}

/// Find active runs that have overlapping refs with the new push.
pub fn find_overlapping_runs<'a>(new_refs: &[RefUpdate], active_runs: &'a [Run]) -> Vec<&'a Run> {
    active_runs
        .iter()
        .filter(|run| {
            run.ref_updates
                .iter()
                .any(|r| new_refs.iter().any(|nr| nr.ref_name == r.ref_name))
        })
        .collect()
}

/// Supersede old runs that have overlapping refs with the new push.
///
/// This marks old runs as Superseded so the user only sees the latest run.
/// Returns the full Run structs of superseded runs so callers can inherit
/// their base_sha (fixing the superseding gap where changes would be
/// forwarded without review).
pub fn supersede_overlapping_runs(
    db: &Database,
    repo_id: &str,
    new_refs: &[RefUpdate],
) -> Result<Vec<Run>, airlock_core::AirlockError> {
    let active_runs = db.list_active_runs(repo_id)?;
    let overlapping = find_overlapping_runs(new_refs, &active_runs);

    let mut superseded_runs = Vec::new();
    for run in overlapping {
        info!(
            "Superseding run {} (overlapping refs with new push)",
            run.id
        );
        // Clone before marking as superseded so we preserve the original base_sha
        superseded_runs.push(run.clone());
        db.mark_run_superseded(&run.id)?;
    }

    Ok(superseded_runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_ref_updates_new_ref() {
        let mut existing = vec![RefUpdate {
            ref_name: "refs/heads/main".to_string(),
            old_sha: "aaa".to_string(),
            new_sha: "bbb".to_string(),
        }];

        let new = vec![RefUpdate {
            ref_name: "refs/heads/feature".to_string(),
            old_sha: "ccc".to_string(),
            new_sha: "ddd".to_string(),
        }];

        merge_ref_updates(&mut existing, new);

        assert_eq!(existing.len(), 2);
        assert!(existing.iter().any(|r| r.ref_name == "refs/heads/main"));
        assert!(existing.iter().any(|r| r.ref_name == "refs/heads/feature"));
    }

    #[test]
    fn test_merge_ref_updates_same_ref() {
        let mut existing = vec![RefUpdate {
            ref_name: "refs/heads/main".to_string(),
            old_sha: "aaa".to_string(),
            new_sha: "bbb".to_string(),
        }];

        let new = vec![RefUpdate {
            ref_name: "refs/heads/main".to_string(),
            old_sha: "bbb".to_string(),
            new_sha: "ccc".to_string(),
        }];

        merge_ref_updates(&mut existing, new);

        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0].old_sha, "aaa"); // Keep original old_sha
        assert_eq!(existing[0].new_sha, "ccc"); // Use new new_sha
    }

    #[test]
    fn test_has_overlapping_refs() {
        let new_refs = vec![RefUpdate {
            ref_name: "refs/heads/main".to_string(),
            old_sha: "aaa".to_string(),
            new_sha: "bbb".to_string(),
        }];

        let active_runs = vec![Run {
            id: "run1".to_string(),
            repo_id: "repo1".to_string(),
            ref_updates: vec![RefUpdate {
                ref_name: "refs/heads/main".to_string(),
                old_sha: "000".to_string(),
                new_sha: "aaa".to_string(),
            }],
            error: None,
            superseded: false,
            created_at: 1000,
            branch: String::new(),
            base_sha: String::new(),
            head_sha: String::new(),
            current_step: None,
            updated_at: 1000,
            workflow_file: String::new(),
            workflow_name: None,
        }];

        assert!(has_overlapping_refs(&new_refs, &active_runs));
    }

    #[test]
    fn test_no_overlapping_refs() {
        let new_refs = vec![RefUpdate {
            ref_name: "refs/heads/feature".to_string(),
            old_sha: "aaa".to_string(),
            new_sha: "bbb".to_string(),
        }];

        let active_runs = vec![Run {
            id: "run1".to_string(),
            repo_id: "repo1".to_string(),
            ref_updates: vec![RefUpdate {
                ref_name: "refs/heads/main".to_string(),
                old_sha: "000".to_string(),
                new_sha: "aaa".to_string(),
            }],
            error: None,
            superseded: false,
            created_at: 1000,
            branch: String::new(),
            base_sha: String::new(),
            head_sha: String::new(),
            current_step: None,
            updated_at: 1000,
            workflow_file: String::new(),
            workflow_name: None,
        }];

        assert!(!has_overlapping_refs(&new_refs, &active_runs));
    }

    #[tokio::test]
    async fn test_coalescer_first_push() {
        let coalescer = PushCoalescer::new();

        let refs = vec![RefUpdate {
            ref_name: "refs/heads/main".to_string(),
            old_sha: "aaa".to_string(),
            new_sha: "bbb".to_string(),
        }];

        // First push should be recorded, not returned
        let result = coalescer.record_push("repo1", refs).await;
        assert!(result.is_none());

        // Should have one pending push
        let pending = coalescer.pending.lock().await;
        assert!(pending.contains_key("repo1"));
    }

    #[tokio::test]
    async fn test_coalescer_merge_pushes() {
        let coalescer = PushCoalescer::new();

        let refs1 = vec![RefUpdate {
            ref_name: "refs/heads/main".to_string(),
            old_sha: "aaa".to_string(),
            new_sha: "bbb".to_string(),
        }];

        let refs2 = vec![RefUpdate {
            ref_name: "refs/heads/main".to_string(),
            old_sha: "bbb".to_string(),
            new_sha: "ccc".to_string(),
        }];

        coalescer.record_push("repo1", refs1).await;
        coalescer.record_push("repo1", refs2).await;

        // Should have merged the pushes
        let pending = coalescer.pending.lock().await;
        let push = pending.get("repo1").unwrap();
        assert_eq!(push.ref_updates.len(), 1);
        assert_eq!(push.ref_updates[0].old_sha, "aaa");
        assert_eq!(push.ref_updates[0].new_sha, "ccc");
    }
}
