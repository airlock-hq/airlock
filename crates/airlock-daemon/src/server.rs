//! Daemon server implementation.
//!
//! This module provides the main IPC server that listens on a Unix domain socket
//! (or Windows named pipe) and dispatches JSON-RPC requests to the appropriate handlers.

use crate::handlers::{
    cleanup_stale_run_refs, detect_and_process_missed_pushes, dispatch, process_ready_pushes,
    HandlerContext,
};
use crate::ipc::{error_codes, methods, Notification, Request, Response};
use airlock_core::{AirlockPaths, Database, JobStatus, StepStatus};
use anyhow::{Context, Result};
use interprocess::local_socket::{
    tokio::{prelude::*, Stream},
    ListenerOptions,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;

/// The main daemon server.
pub struct Server {
    paths: AirlockPaths,
    /// Sender for shutdown signal.
    shutdown_tx: watch::Sender<bool>,
    /// Receiver for shutdown signal.
    shutdown_rx: watch::Receiver<bool>,
}

impl Server {
    /// Create a new server instance.
    pub fn new(paths: AirlockPaths) -> Result<Self> {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        Ok(Self {
            paths,
            shutdown_tx,
            shutdown_rx,
        })
    }

    /// Create a platform-specific IPC listener.
    #[cfg(unix)]
    fn create_listener(
        &self,
        socket_name: &str,
    ) -> Result<interprocess::local_socket::tokio::Listener> {
        let name = socket_name
            .to_fs_name::<GenericFilePath>()
            .context("Failed to create socket name")?;

        ListenerOptions::new()
            .name(name)
            .create_tokio()
            .context("Failed to create Unix socket listener")
    }

    /// Create a platform-specific IPC listener.
    #[cfg(windows)]
    fn create_listener(
        &self,
        socket_name: &str,
    ) -> Result<interprocess::local_socket::tokio::Listener> {
        let name = socket_name
            .to_ns_name::<GenericNamespaced>()
            .context("Failed to create named pipe name")?;

        ListenerOptions::new()
            .name(name)
            .create_tokio()
            .context("Failed to create named pipe listener")
    }

    /// Run the server.
    pub async fn run(&self) -> Result<()> {
        let socket_name_str = self.paths.socket_name();

        info!("Server starting on socket: {}", socket_name_str);

        // Remove stale socket if it exists (Unix only - Windows named pipes are managed by the OS)
        #[cfg(unix)]
        {
            let socket_path = self.paths.socket();
            if socket_path.exists() {
                std::fs::remove_file(&socket_path).context("Failed to remove stale socket file")?;
                debug!("Removed stale socket file");
            }
        }

        // Initialize database
        let db = Database::open(&self.paths.database()).context("Failed to open database")?;
        info!("Database initialized");

        // Handle orphaned runs from previous daemon crash
        // We check all runs and find those that were actually in progress.
        //
        // Runs can be in these states:
        // 1. Awaiting approval - should NOT be marked as failed, just left paused
        // 2. Actively running a stage - were interrupted and should be marked as failed
        // 3. Have pending stages (not started yet) - were interrupted, mark as failed
        // 4. Already completed (all stages final) - skip, nothing to do
        match db.list_all_runs(None) {
            Ok(all_runs) => {
                let mut orphaned_count = 0;
                for run in &all_runs {
                    // Check stage results to determine the run's actual state
                    match db.get_step_results_for_run(&run.id) {
                        Ok(stages) => {
                            // Skip runs that are already completed (all stages have final status)
                            if run.is_completed(&stages) {
                                continue;
                            }

                            // If any stage is awaiting approval, the run was paused - leave it alone
                            if run.is_awaiting_approval(&stages) {
                                info!(
                                    "Run {} was awaiting approval - leaving it paused for user to resume",
                                    run.id
                                );
                                continue;
                            }

                            // At this point, the run was actually in progress (not completed, not awaiting approval)
                            orphaned_count += 1;

                            // Mark any Running steps as Failed
                            for stage in &stages {
                                if stage.status == StepStatus::Running {
                                    let mut failed_stage = stage.clone();
                                    failed_stage.status = StepStatus::Failed;
                                    failed_stage.error = Some(
                                        "Stage interrupted: daemon was restarted while stage was running".to_string()
                                    );
                                    if let Err(e) = db.update_step_result(&failed_stage) {
                                        warn!(
                                            "Failed to mark stage {} as failed: {}",
                                            stage.name, e
                                        );
                                    }
                                }
                            }

                            // Mark any remaining Pending steps as Skipped so the run
                            // is considered fully completed and won't be re-triggered
                            for stage in &stages {
                                if stage.status == StepStatus::Pending {
                                    let mut skipped_stage = stage.clone();
                                    skipped_stage.status = StepStatus::Skipped;
                                    if let Err(e) = db.update_step_result(&skipped_stage) {
                                        warn!(
                                            "Failed to mark stage {} as skipped: {}",
                                            stage.name, e
                                        );
                                    }
                                }
                            }

                            // Mark any non-final jobs as Failed/Skipped
                            if let Ok(jobs) = db.get_job_results_for_run(&run.id) {
                                for job in &jobs {
                                    if !job.status.is_final() {
                                        let new_status = if job.status == JobStatus::Running {
                                            JobStatus::Failed
                                        } else {
                                            JobStatus::Skipped
                                        };
                                        if let Err(e) = db.update_job_status(
                                            &job.id,
                                            new_status,
                                            None,
                                            None,
                                            Some("Pipeline interrupted: daemon was restarted"),
                                        ) {
                                            warn!(
                                                "Failed to mark job {} as {:?}: {}",
                                                job.job_key, new_status, e
                                            );
                                        }
                                    }
                                }
                            }

                            // Mark the run as failed
                            let error_msg =
                                "Pipeline interrupted: daemon was restarted while run was in progress";
                            if let Err(e) = db.update_run_error(&run.id, Some(error_msg)) {
                                warn!("Failed to mark orphaned run {} as failed: {}", run.id, e);
                            } else {
                                info!("Marked orphaned run {} as failed", run.id);
                            }
                        }
                        Err(e) => {
                            // Can't determine state - this run might be orphaned, but we can't tell
                            // Log and skip rather than potentially corrupting a valid run
                            warn!(
                                "Failed to get stage results for run {}, skipping: {}",
                                run.id, e
                            );
                        }
                    }
                }

                if orphaned_count > 0 {
                    info!(
                        "Processed {} orphaned run(s) from previous daemon session",
                        orphaned_count
                    );
                }
            }
            Err(e) => {
                warn!("Failed to check for orphaned runs: {}", e);
            }
        }

        // Re-install hooks for all enrolled repos to ensure they have the latest
        // hook scripts (e.g., with push marker ref creation support)
        {
            match db.list_repos() {
                Ok(repos) => {
                    for repo in &repos {
                        if let Err(e) = airlock_core::git::hooks::install_hooks(&repo.gate_path) {
                            warn!("Failed to re-install hooks for repo {}: {}", repo.id, e);
                        } else {
                            debug!("Re-installed hooks for repo {}", repo.id);
                        }
                    }
                    if !repos.is_empty() {
                        info!("Re-installed hooks for {} enrolled repo(s)", repos.len());
                    }
                }
                Err(e) => {
                    warn!("Failed to list repos for hook re-installation: {}", e);
                }
            }
        }

        // Create handler context with shutdown sender
        let ctx = Arc::new(HandlerContext::new(
            self.paths.clone(),
            db,
            self.shutdown_tx.clone(),
        ));

        // Initialize worktree pool from disk state (recover from crash/restart)
        {
            let db = ctx.db.lock().await;
            if let Err(e) = ctx.worktree_pool.init_from_disk(&ctx.paths, &db).await {
                warn!("Failed to initialize worktree pool from disk: {}", e);
            }
        }

        // Create the listener with platform-specific socket name
        let listener = self.create_listener(&socket_name_str)?;

        info!("Listening for connections on {}", socket_name_str);

        // Clean up protective refs for completed/superseded runs
        cleanup_stale_run_refs(Arc::clone(&ctx)).await;

        // Detect and process any pushes that were missed while daemon was down
        // This handles the case where user pushed to gate but daemon wasn't running
        detect_and_process_missed_pushes(Arc::clone(&ctx)).await;

        // Spawn a background task to process ready pushes periodically
        // This ensures coalesced pushes are processed even if no new notifications arrive
        let coalescer_ctx = Arc::clone(&ctx);
        let mut coalescer_shutdown_rx = self.shutdown_rx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        process_ready_pushes(Arc::clone(&coalescer_ctx)).await;
                    }
                    _ = coalescer_shutdown_rx.changed() => {
                        if *coalescer_shutdown_rx.borrow() {
                            debug!("Coalescer task shutting down");
                            break;
                        }
                    }
                }
            }
        });

        // Accept connections in a loop until shutdown is requested
        let mut shutdown_rx = self.shutdown_rx.clone();
        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok(stream) => {
                            let ctx = Arc::clone(&ctx);
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(ctx, stream).await {
                                    error!("Connection handler error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept connection: {}", e);
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("Shutdown signal received, stopping server...");
                        break;
                    }
                }
            }
        }

        // Clean up socket file on shutdown (Unix only)
        #[cfg(unix)]
        {
            let socket_path = self.paths.socket();
            if socket_path.exists() {
                if let Err(e) = std::fs::remove_file(&socket_path) {
                    warn!("Failed to remove socket file on shutdown: {}", e);
                } else {
                    debug!("Removed socket file on shutdown");
                }
            }
        }

        info!("Server stopped");
        Ok(())
    }
}

/// Handle a single client connection.
async fn handle_connection(ctx: Arc<HandlerContext>, stream: Stream) -> Result<()> {
    debug!("New connection accepted");

    // Split the stream into read and write halves
    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    // Read requests line by line (each line is a JSON-RPC request)
    loop {
        line.clear();

        match reader.read_line(&mut line).await {
            Ok(0) => {
                // EOF - client disconnected
                debug!("Client disconnected");
                break;
            }
            Ok(_) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                debug!("Received request: {}", line);

                // Parse the request
                let request = match serde_json::from_str::<Request>(line) {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("Failed to parse request: {}", e);
                        let response = Response::error(
                            serde_json::Value::Null,
                            error_codes::PARSE_ERROR,
                            format!("Parse error: {}", e),
                        );
                        let response_json = serde_json::to_string(&response)?;
                        writer.write_all(response_json.as_bytes()).await?;
                        writer.write_all(b"\n").await?;
                        writer.flush().await?;
                        continue;
                    }
                };

                // Check JSON-RPC version
                if request.jsonrpc != "2.0" {
                    let response = Response::error(
                        request.id,
                        error_codes::INVALID_REQUEST,
                        "Invalid JSON-RPC version".to_string(),
                    );
                    let response_json = serde_json::to_string(&response)?;
                    writer.write_all(response_json.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await?;
                    continue;
                }

                // Check if this is a subscribe request - handle specially
                if request.method == methods::SUBSCRIBE {
                    info!("Client subscribing to events");
                    // Send success response
                    let response =
                        Response::success(request.id, serde_json::json!({"subscribed": true}));
                    let response_json = serde_json::to_string(&response)?;
                    writer.write_all(response_json.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await?;

                    // Now switch to event streaming mode
                    handle_event_subscription(ctx, writer).await?;
                    return Ok(());
                }

                // Dispatch to handler
                let response = dispatch(Arc::clone(&ctx), request).await;

                // Serialize and send response
                let response_json = serde_json::to_string(&response)?;
                debug!("Sending response: {}", response_json);

                writer.write_all(response_json.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;
            }
            Err(e) => {
                error!("Read error: {}", e);
                break;
            }
        }
    }

    Ok(())
}

/// Handle event subscription - stream events to the client until disconnection.
async fn handle_event_subscription<W>(ctx: Arc<HandlerContext>, mut writer: W) -> Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut event_rx = ctx.subscribe();

    loop {
        match event_rx.recv().await {
            Ok(event) => {
                // Convert event to JSON-RPC notification
                let notification = Notification::event(&event);
                let notification_json = match serde_json::to_string(&notification) {
                    Ok(json) => json,
                    Err(e) => {
                        warn!("Failed to serialize event: {}", e);
                        continue;
                    }
                };

                // Send to client
                if let Err(e) = writer.write_all(notification_json.as_bytes()).await {
                    debug!("Event subscriber disconnected: {}", e);
                    break;
                }
                if let Err(e) = writer.write_all(b"\n").await {
                    debug!("Event subscriber disconnected: {}", e);
                    break;
                }
                if let Err(e) = writer.flush().await {
                    debug!("Event subscriber disconnected: {}", e);
                    break;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                info!("Event channel closed, ending subscription");
                break;
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                warn!("Event subscriber lagged, missed {} events", count);
                // Continue - we'll get the next event
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    // Integration tests would go here, but they require a running server
    // and are better suited for separate integration test files.
}
