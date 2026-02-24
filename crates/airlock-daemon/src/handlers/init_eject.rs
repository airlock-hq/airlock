//! Init and eject handlers.
//!
//! Handles repository enrollment and unenrollment from Airlock.

use super::util::parse_params;
use super::HandlerContext;
use crate::ipc::{error_codes, EjectParams, EjectResult, InitParams, InitResult, Response};
use airlock_core::init;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

/// Handle the `init` method.
pub async fn handle_init(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    let params: InitParams = match parse_params(params, &id) {
        Ok(p) => p,
        Err(r) => return r,
    };

    let working_path = Path::new(&params.working_path);

    // Validate working path exists
    if !working_path.exists() {
        return Response::error(
            id,
            error_codes::INVALID_PARAMS,
            format!("Working path does not exist: {}", params.working_path),
        );
    }

    // Delegate to shared core logic
    let db = ctx.db.lock().await;
    match init::init_repo(working_path, &ctx.paths, &db) {
        Ok(outcome) => {
            info!(
                "Initialized Airlock for repo {} at {}",
                outcome.repo_id,
                working_path.display()
            );

            let result = InitResult {
                repo_id: outcome.repo_id,
                gate_path: outcome.gate_path.to_string_lossy().to_string(),
            };

            Response::success(id, serde_json::to_value(result).unwrap())
        }
        Err(e) => Response::error(id, error_codes::INTERNAL_ERROR, format!("{:#}", e)),
    }
}

/// Handle the `eject` method.
pub async fn handle_eject(
    ctx: Arc<HandlerContext>,
    params: serde_json::Value,
    id: serde_json::Value,
) -> Response {
    let params: EjectParams = match parse_params(params, &id) {
        Ok(p) => p,
        Err(r) => return r,
    };

    let working_path = Path::new(&params.working_path);

    // Delegate to shared core logic
    let db = ctx.db.lock().await;
    match init::eject_repo(working_path, &ctx.paths, &db) {
        Ok(outcome) => {
            info!("Ejected from Airlock: {}", working_path.display());

            let result = EjectResult {
                upstream_url: outcome.upstream_url,
            };
            Response::success(id, serde_json::to_value(result).unwrap())
        }
        Err(e) => Response::error(id, error_codes::INTERNAL_ERROR, format!("{:#}", e)),
    }
}
