//! Event bridge for streaming daemon events to the frontend.
//!
//! This module connects to the daemon's subscribe endpoint and forwards
//! events to the Tauri frontend via the app's event system.

use airlock_core::ipc::AirlockEvent;
use airlock_core::AirlockPaths;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[cfg(unix)]
use interprocess::local_socket::tokio::prelude::*;
#[cfg(unix)]
use interprocess::local_socket::{tokio::Stream, GenericFilePath};

#[cfg(windows)]
use interprocess::local_socket::tokio::prelude::*;
#[cfg(windows)]
use interprocess::local_socket::{tokio::Stream, GenericNamespaced};

/// JSON-RPC notification from the daemon.
#[derive(Debug, Deserialize)]
struct Notification {
    #[allow(dead_code)]
    jsonrpc: String,
    method: String,
    params: serde_json::Value,
}

/// JSON-RPC request for subscribing.
#[derive(Debug, Serialize)]
struct SubscribeRequest {
    jsonrpc: &'static str,
    method: &'static str,
    params: serde_json::Value,
    id: u32,
}

/// JSON-RPC response.
#[derive(Debug, Deserialize)]
struct Response {
    #[allow(dead_code)]
    result: Option<serde_json::Value>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    #[allow(dead_code)]
    code: i32,
    message: String,
}

/// State for the event bridge.
pub struct EventBridgeState {
    /// Whether the bridge should be running.
    running: AtomicBool,
    /// Request ID counter.
    request_id: AtomicU32,
}

impl EventBridgeState {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(true),
            request_id: AtomicU32::new(1),
        }
    }

    /// Stop the event bridge.
    #[allow(dead_code)]
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    fn next_id(&self) -> u32 {
        self.request_id.fetch_add(1, Ordering::SeqCst)
    }
}

impl Default for EventBridgeState {
    fn default() -> Self {
        Self::new()
    }
}

/// Connect to the daemon socket.
#[cfg(unix)]
async fn connect_to_daemon(paths: &AirlockPaths) -> Result<Stream, String> {
    let socket_name = paths.socket_name();
    let name = socket_name
        .to_fs_name::<GenericFilePath>()
        .map_err(|e| format!("Failed to create socket name: {}", e))?;

    Stream::connect(name)
        .await
        .map_err(|e| format!("Failed to connect to daemon: {}", e))
}

#[cfg(windows)]
async fn connect_to_daemon(paths: &AirlockPaths) -> Result<Stream, String> {
    let socket_name = paths.socket_name();
    let name = socket_name
        .to_ns_name::<GenericNamespaced>()
        .map_err(|e| format!("Failed to create socket name: {}", e))?;

    Stream::connect(name)
        .await
        .map_err(|e| format!("Failed to connect to daemon: {}", e))
}

/// Run the event bridge loop.
///
/// This connects to the daemon, subscribes to events, and forwards them
/// to the Tauri frontend. It will automatically reconnect on disconnect.
pub async fn run_event_bridge<R: tauri::Runtime>(app_handle: tauri::AppHandle<R>) {
    let paths = AirlockPaths::default();
    let state = Arc::new(EventBridgeState::new());

    // Store state in app for potential cleanup
    // Note: In a real implementation, you might want to store this in Tauri's state management

    info!("Event bridge starting...");

    // Reconnect loop with backoff
    let mut reconnect_delay = Duration::from_secs(1);
    let max_reconnect_delay = Duration::from_secs(30);

    while state.running.load(Ordering::SeqCst) {
        match run_subscription_loop(&app_handle, &paths, &state).await {
            Ok(()) => {
                // Clean exit
                info!("Event bridge stopped cleanly");
                break;
            }
            Err(e) => {
                warn!(
                    "Event bridge connection lost: {}. Reconnecting in {:?}...",
                    e, reconnect_delay
                );

                // Wait before reconnecting
                tokio::time::sleep(reconnect_delay).await;

                // Exponential backoff
                reconnect_delay = std::cmp::min(reconnect_delay * 2, max_reconnect_delay);
            }
        }
    }
}

/// Run a single subscription session.
async fn run_subscription_loop<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    paths: &AirlockPaths,
    state: &EventBridgeState,
) -> Result<(), String> {
    // Connect to daemon
    let stream = connect_to_daemon(paths).await?;
    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);

    // Send subscribe request
    let request = SubscribeRequest {
        jsonrpc: "2.0",
        method: "subscribe",
        params: serde_json::json!({}),
        id: state.next_id(),
    };

    let request_json = serde_json::to_string(&request)
        .map_err(|e| format!("Failed to serialize subscribe request: {}", e))?;

    writer
        .write_all(request_json.as_bytes())
        .await
        .map_err(|e| format!("Failed to send subscribe request: {}", e))?;
    writer
        .write_all(b"\n")
        .await
        .map_err(|e| format!("Failed to send newline: {}", e))?;
    writer
        .flush()
        .await
        .map_err(|e| format!("Failed to flush: {}", e))?;

    // Read subscribe response
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .await
        .map_err(|e| format!("Failed to read subscribe response: {}", e))?;

    let response: Response = serde_json::from_str(&line)
        .map_err(|e| format!("Failed to parse subscribe response: {}", e))?;

    if let Some(error) = response.error {
        return Err(format!("Subscribe failed: {}", error.message));
    }

    info!("Event bridge subscribed to daemon events");

    // Reset backoff on successful connection
    // (handled by caller)

    // Read events
    loop {
        if !state.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                // EOF - daemon disconnected
                return Err("Daemon disconnected".to_string());
            }
            Ok(_) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                debug!("Received event: {}", line);

                // Parse notification
                match serde_json::from_str::<Notification>(line) {
                    Ok(notification) => {
                        if notification.method == "event" {
                            // Parse the event
                            match serde_json::from_value::<AirlockEvent>(notification.params) {
                                Ok(event) => {
                                    // Forward to frontend via Tauri events
                                    emit_event_to_frontend(app_handle, &event);
                                }
                                Err(e) => {
                                    warn!("Failed to parse event params: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse notification: {}", e);
                    }
                }
            }
            Err(e) => {
                return Err(format!("Read error: {}", e));
            }
        }
    }
}

/// Emit an event to the Tauri frontend.
fn emit_event_to_frontend<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    event: &AirlockEvent,
) {
    // Emit a general event with the full payload
    if let Err(e) = app_handle.emit("airlock://event", event) {
        warn!("Failed to emit event to frontend: {}", e);
    }

    // Send OS notifications
    match event {
        AirlockEvent::RunCreated { branch, .. } => {
            use tauri_plugin_notification::NotificationExt;
            if let Err(e) = app_handle
                .notification()
                .builder()
                .title("Airlock")
                .body(format!("Push received on {}", branch))
                .show()
            {
                warn!("Failed to send push notification: {}", e);
            }
        }
        AirlockEvent::StepCompleted {
            step_name,
            status,
            branch,
            ..
        } if status == "awaiting_approval" => {
            use tauri_plugin_notification::NotificationExt;
            let display_branch = branch.strip_prefix("refs/heads/").unwrap_or(branch);
            if let Err(e) = app_handle
                .notification()
                .builder()
                .title("Airlock - Approval Required")
                .body(format!(
                    "Step '{}' is awaiting approval on {}",
                    step_name, display_branch
                ))
                .show()
            {
                warn!("Failed to send approval notification: {}", e);
            }
        }
        AirlockEvent::RunCompleted {
            success, branch, ..
        } => {
            use tauri_plugin_notification::NotificationExt;
            let display_branch = branch.strip_prefix("refs/heads/").unwrap_or(branch);
            let (title, body) = if *success {
                (
                    "Airlock - Pipeline Passed",
                    format!("Pipeline completed successfully on {}", display_branch),
                )
            } else {
                (
                    "Airlock - Pipeline Failed",
                    format!("Pipeline failed on {}", display_branch),
                )
            };
            if let Err(e) = app_handle
                .notification()
                .builder()
                .title(title)
                .body(body)
                .show()
            {
                warn!("Failed to send pipeline notification: {}", e);
            }
        }
        AirlockEvent::RunSuperseded { .. } => {
            // No notification for superseded runs — they're silently replaced
        }
        _ => {}
    }

    // Also emit specific event types for easier subscription
    let (event_name, payload) = match event {
        AirlockEvent::RunCreated {
            repo_id,
            run_id,
            branch,
        } => (
            "airlock://run-created",
            serde_json::json!({ "repo_id": repo_id, "run_id": run_id, "branch": branch }),
        ),
        AirlockEvent::RunUpdated {
            repo_id,
            run_id,
            status,
        } => (
            "airlock://run-updated",
            serde_json::json!({ "repo_id": repo_id, "run_id": run_id, "status": status }),
        ),
        AirlockEvent::JobStarted {
            repo_id,
            run_id,
            job_key,
        } => (
            "airlock://job-started",
            serde_json::json!({ "repo_id": repo_id, "run_id": run_id, "job_key": job_key }),
        ),
        AirlockEvent::JobCompleted {
            repo_id,
            run_id,
            job_key,
            status,
        } => (
            "airlock://job-completed",
            serde_json::json!({ "repo_id": repo_id, "run_id": run_id, "job_key": job_key, "status": status }),
        ),
        AirlockEvent::StepStarted {
            repo_id,
            run_id,
            job_key,
            step_name,
        } => (
            "airlock://step-started",
            serde_json::json!({ "repo_id": repo_id, "run_id": run_id, "job_key": job_key, "step_name": step_name }),
        ),
        AirlockEvent::StepCompleted {
            repo_id,
            run_id,
            job_key,
            step_name,
            status,
            branch,
        } => (
            "airlock://step-completed",
            serde_json::json!({ "repo_id": repo_id, "run_id": run_id, "job_key": job_key, "step_name": step_name, "status": status, "branch": branch }),
        ),
        AirlockEvent::RunCompleted {
            repo_id,
            run_id,
            success,
            branch,
        } => (
            "airlock://run-completed",
            serde_json::json!({ "repo_id": repo_id, "run_id": run_id, "success": success, "branch": branch }),
        ),
        AirlockEvent::RunSuperseded { repo_id, run_id } => (
            "airlock://run-superseded",
            serde_json::json!({ "repo_id": repo_id, "run_id": run_id }),
        ),
        AirlockEvent::LogChunk {
            repo_id,
            run_id,
            job_key,
            step_name,
            stream,
            content,
        } => (
            "airlock://log-chunk",
            serde_json::json!({ "repo_id": repo_id, "run_id": run_id, "job_key": job_key, "step_name": step_name, "stream": stream, "content": content }),
        ),
    };

    if let Err(e) = app_handle.emit(event_name, payload) {
        warn!("Failed to emit {} event to frontend: {}", event_name, e);
    }
}
