//! IPC request handlers.
//!
//! This module contains the implementation of all JSON-RPC method handlers.

mod config;
mod forward;
mod init_eject;
mod pipeline;
mod push;
mod runs;
mod status;
mod steps;
mod sync;
mod util;

use crate::ipc::{error_codes, methods, AirlockEvent, Response, ShutdownResult};
use crate::push_coalescer::PushCoalescer;
use crate::run_queue::RunQueue;
use airlock_core::{AirlockPaths, Database};
use std::sync::Arc;
use tokio::sync::{broadcast, watch, Mutex};
use tracing::{error, info, trace};

// Re-export public items
pub use push::{cleanup_stale_run_refs, detect_and_process_missed_pushes, process_ready_pushes};

use crate::ipc::Request;

/// Channel capacity for the event broadcast channel.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// Handler context shared across all handlers.
pub struct HandlerContext {
    /// Airlock paths.
    pub paths: AirlockPaths,

    /// Database connection (wrapped in Mutex for thread safety).
    pub db: Mutex<Database>,

    /// Push coalescer for debouncing and deduplicating rapid pushes.
    pub coalescer: PushCoalescer,

    /// Per-repo run serialization queue.
    pub run_queue: RunQueue,

    /// Shutdown signal sender for graceful shutdown.
    pub shutdown_tx: watch::Sender<bool>,

    /// Event broadcast sender for real-time updates.
    pub event_tx: broadcast::Sender<AirlockEvent>,
}

impl HandlerContext {
    /// Create a new handler context.
    pub fn new(paths: AirlockPaths, db: Database, shutdown_tx: watch::Sender<bool>) -> Self {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            paths,
            db: Mutex::new(db),
            coalescer: PushCoalescer::new(),
            run_queue: RunQueue::new(),
            shutdown_tx,
            event_tx,
        }
    }

    /// Emit an event to all subscribers.
    pub fn emit(&self, event: AirlockEvent) {
        // Log at trace level to avoid spam
        trace!("Emitting event: {:?}", event);
        // Ignore send errors - they just mean no subscribers are connected
        let _ = self.event_tx.send(event);
    }

    /// Subscribe to events. Returns a receiver for the event stream.
    pub fn subscribe(&self) -> broadcast::Receiver<AirlockEvent> {
        self.event_tx.subscribe()
    }
}

/// Dispatch a request to the appropriate handler.
pub async fn dispatch(ctx: Arc<HandlerContext>, request: Request) -> Response {
    let id = request.id.clone();

    match request.method.as_str() {
        methods::INIT => init_eject::handle_init(ctx, request.params, id).await,
        methods::EJECT => init_eject::handle_eject(ctx, request.params, id).await,
        methods::SYNC => sync::handle_sync(ctx, request.params, id).await,
        methods::SYNC_ALL => sync::handle_sync_all(ctx, id).await,
        methods::STATUS => status::handle_status(ctx, request.params, id).await,
        methods::HEALTH => status::handle_health(ctx, id).await,
        methods::GET_RUNS => runs::handle_get_runs(ctx, request.params, id).await,
        methods::GET_RUN_DETAIL => runs::handle_get_run_detail(ctx, request.params, id).await,
        methods::MARK_FORWARDED => forward::handle_mark_forwarded(ctx, request.params, id).await,
        methods::PUSH_RECEIVED => {
            let result = push::handle_push_received(ctx, request.params).await;
            Response::success(id, serde_json::to_value(result).unwrap())
        }
        methods::FETCH_NOTIFICATION => {
            sync::handle_fetch_notification(ctx, request.params, id).await
        }
        methods::SHUTDOWN => handle_shutdown(ctx, id).await,
        methods::GET_REPOS => status::handle_get_repos(ctx, id).await,
        methods::REPROCESS_RUN => runs::handle_reprocess_run(ctx, request.params, id).await,
        methods::GET_CONFIG => config::handle_get_config(ctx, request.params, id).await,
        methods::UPDATE_CONFIG => config::handle_update_config(ctx, request.params, id).await,
        // Step-based pipeline handlers
        methods::APPROVE_STEP => steps::handle_approve_step(ctx, request.params, id).await,
        methods::GET_RUN_DIFF => steps::handle_get_run_diff(ctx, request.params, id).await,
        methods::APPLY_PATCHES => steps::handle_apply_patches(ctx, request.params, id).await,
        _ => Response::error(
            id,
            error_codes::METHOD_NOT_FOUND,
            format!("Method '{}' not found", request.method),
        ),
    }
}

/// Handle the `shutdown` method.
async fn handle_shutdown(ctx: Arc<HandlerContext>, id: serde_json::Value) -> Response {
    info!("Shutdown requested via IPC");

    // Send the shutdown signal
    if let Err(e) = ctx.shutdown_tx.send(true) {
        error!("Failed to send shutdown signal: {}", e);
        return Response::error(
            id,
            error_codes::INTERNAL_ERROR,
            format!("Failed to initiate shutdown: {}", e),
        );
    }

    let result = ShutdownResult { acknowledged: true };
    Response::success(id, serde_json::to_value(result).unwrap())
}
