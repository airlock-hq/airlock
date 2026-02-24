//! Mark-forwarded handler.
//!
//! Handles bookkeeping after a push stage has successfully forwarded changes
//! to upstream. Updates tracking refs, cleans up protective refs, and clears
//! run errors — but does NOT perform any git push (that already happened in
//! the push stage).

use super::util::parse_params;
use super::HandlerContext;
use crate::ipc::{error_codes, MarkForwardedParams, MarkForwardedResult, Response};
use airlock_core::git;
use std::sync::Arc;
use tracing::{info, warn};

/// Handle the `mark_forwarded` method.
///
/// Called by the push stage after a successful `git push` to upstream.
/// Performs bookkeeping only — no git push.
pub async fn handle_mark_forwarded(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    let params: MarkForwardedParams = match parse_params(params, &id) {
        Ok(p) => p,
        Err(r) => return r,
    };

    // Get run
    let run = {
        let db = ctx.db.lock().await;
        match db.get_run(&params.run_id) {
            Ok(Some(r)) => r,
            Ok(None) => {
                return Response::error(
                    id,
                    error_codes::RUN_NOT_FOUND,
                    format!("Run not found: {}", params.run_id),
                )
            }
            Err(e) => {
                return Response::error(
                    id,
                    error_codes::DATABASE_ERROR,
                    format!("Failed to query database: {}", e),
                )
            }
        }
    };

    // Get repo
    let repo = {
        let db = ctx.db.lock().await;
        match db.get_repo(&run.repo_id) {
            Ok(Some(r)) => r,
            Ok(None) => {
                return Response::error(
                    id,
                    error_codes::REPO_NOT_FOUND,
                    format!("Repository not found: {}", run.repo_id),
                )
            }
            Err(e) => {
                return Response::error(
                    id,
                    error_codes::DATABASE_ERROR,
                    format!("Failed to query database: {}", e),
                )
            }
        }
    };

    // Update remote tracking ref in the gate to reflect the forwarded state.
    if let Some(branch) = params.ref_name.strip_prefix("refs/heads/") {
        let tracking_ref = format!("refs/remotes/origin/{}", branch);
        if let Err(e) = git::update_ref(&repo.gate_path, &tracking_ref, &params.sha) {
            warn!("Failed to update tracking ref {}: {}", tracking_ref, e);
        }
    }

    // Clear any error on successful forward
    {
        let db = ctx.db.lock().await;
        if let Err(e) = db.update_run_error(&run.id, None) {
            warn!("Failed to clear run error: {}", e);
        }
    }

    // Delete protective ref now that the run has been forwarded to upstream
    let protective_ref = git::run_ref(&run.id);
    if let Err(e) = git::delete_ref(&repo.gate_path, &protective_ref) {
        // Non-fatal: ref may not exist if it was already cleaned up
        warn!("Failed to delete protective ref for run {}: {}", run.id, e);
    } else {
        info!("Deleted protective ref {} after forward", protective_ref);
    }

    let result = MarkForwardedResult { success: true };
    Response::success(id, serde_json::to_value(result).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::HandlerContext;
    use crate::ipc::MarkForwardedResult;
    use airlock_core::{AirlockPaths, Database, RefUpdate, Repo, Run};
    use tempfile::TempDir;
    use tokio::sync::watch;

    fn create_bare_repo_commit(repo: &git2::Repository) -> String {
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let oid = repo
            .commit(Some("refs/heads/main"), &sig, &sig, "initial", &tree, &[])
            .unwrap();
        oid.to_string()
    }

    /// Happy path: mark_forwarded updates the tracking ref, deletes the
    /// protective ref, clears the run error, and returns success.
    #[tokio::test]
    async fn test_mark_forwarded_happy_path() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path().join("airlock");
        std::fs::create_dir_all(&root).unwrap();

        let paths = AirlockPaths::with_root(root);

        // Create a real bare gate repo
        let gate_path = temp_dir.path().join("gate.git");
        let gate_repo = git2::Repository::init_bare(&gate_path).unwrap();
        gate_repo.remote("origin", "file:///dev/null").unwrap();

        let head_sha = create_bare_repo_commit(&gate_repo);

        // Set up database
        let db = Database::open_in_memory().unwrap();
        let repo = Repo {
            id: "repo1".to_string(),
            working_path: temp_dir.path().to_path_buf(),
            upstream_url: "file:///dev/null".to_string(),
            gate_path: gate_path.clone(),
            last_sync: None,
            created_at: 1704067200,
        };
        db.insert_repo(&repo).unwrap();

        let run = Run {
            id: "run1".to_string(),
            repo_id: "repo1".to_string(),
            ref_updates: vec![RefUpdate {
                ref_name: "refs/heads/main".to_string(),
                old_sha: "0000000000000000000000000000000000000000".to_string(),
                new_sha: head_sha.clone(),
            }],
            branch: "main".to_string(),
            base_sha: "0000000000000000000000000000000000000000".to_string(),
            head_sha: head_sha.clone(),
            current_step: None,
            error: Some("create-pr failed".to_string()),
            superseded: false,
            workflow_file: "main.yml".to_string(),
            workflow_name: None,
            created_at: 1704067200,
            updated_at: 1704067200,
        };
        db.insert_run(&run).unwrap();

        // Create the protective ref (simulating what process_coalesced_push does)
        let protective_ref = git::run_ref("run1");
        git::update_ref(&gate_path, &protective_ref, &head_sha).unwrap();

        // Verify protective ref exists before the call
        let resolved = git::resolve_ref(&gate_path, &protective_ref).unwrap();
        assert_eq!(
            resolved,
            Some(head_sha.clone()),
            "Protective ref should exist before mark_forwarded"
        );

        let (shutdown_tx, _) = watch::channel(false);
        let ctx = Arc::new(HandlerContext::new(paths, db, shutdown_tx));

        // Call handle_mark_forwarded
        let params = serde_json::json!({
            "run_id": "run1",
            "ref_name": "refs/heads/main",
            "sha": head_sha,
        });
        let response = handle_mark_forwarded(ctx.clone(), params, serde_json::json!(1)).await;

        // Should succeed
        assert!(
            response.error.is_none(),
            "Expected success, got error: {:?}",
            response.error
        );
        let result: MarkForwardedResult = serde_json::from_value(response.result.unwrap()).unwrap();
        assert!(result.success);

        // Tracking ref should be updated
        let tracking_ref = format!("refs/remotes/origin/main");
        let tracking = git::resolve_ref(&gate_path, &tracking_ref).unwrap();
        assert_eq!(
            tracking,
            Some(head_sha.clone()),
            "Tracking ref should be updated to the pushed SHA"
        );

        // Protective ref should be deleted
        let protective = git::resolve_ref(&gate_path, &protective_ref).unwrap();
        assert_eq!(
            protective, None,
            "Protective ref should be deleted after mark_forwarded"
        );

        // Run error should be cleared
        {
            let db = ctx.db.lock().await;
            let updated_run = db.get_run("run1").unwrap().unwrap();
            assert_eq!(
                updated_run.error, None,
                "Run error should be cleared after mark_forwarded"
            );
        }
    }

    /// mark_forwarded with a non-existent run returns RUN_NOT_FOUND.
    #[tokio::test]
    async fn test_mark_forwarded_run_not_found() {
        let paths = AirlockPaths::with_root(std::path::PathBuf::from("/tmp/airlock-test-fwd"));
        let db = Database::open_in_memory().unwrap();
        let (shutdown_tx, _) = watch::channel(false);
        let ctx = Arc::new(HandlerContext::new(paths, db, shutdown_tx));

        let params = serde_json::json!({
            "run_id": "nonexistent",
            "ref_name": "refs/heads/main",
            "sha": "abc123",
        });
        let response = handle_mark_forwarded(ctx, params, serde_json::json!(1)).await;

        assert!(response.error.is_some());
        let error = response.error.unwrap();
        assert_eq!(error.code, error_codes::RUN_NOT_FOUND);
    }
}
