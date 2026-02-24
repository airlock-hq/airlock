//! Shared IPC client for communicating with the Airlock daemon.

use anyhow::{Context, Result};
use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::tokio::Stream;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::debug;

use airlock_core::AirlockPaths;

// These imports are only used in release builds for auto-starting the daemon
#[cfg(not(debug_assertions))]
use airlock_core::ServiceManager;
#[cfg(not(debug_assertions))]
use std::time::Duration;
#[cfg(not(debug_assertions))]
use tracing::{info, warn};

#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;

/// JSON-RPC 2.0 request.
#[derive(Debug, Serialize)]
pub struct Request {
    jsonrpc: &'static str,
    method: &'static str,
    params: serde_json::Value,
    id: u32,
}

impl Request {
    pub fn new(method: &'static str) -> Self {
        Self {
            jsonrpc: "2.0",
            method,
            params: serde_json::json!({}),
            id: 1,
        }
    }

    pub fn with_params(method: &'static str, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0",
            method,
            params,
            id: 1,
        }
    }
}

/// JSON-RPC 2.0 response.
#[derive(Debug, Deserialize)]
pub struct Response {
    pub result: Option<serde_json::Value>,
    pub error: Option<RpcError>,
}

/// JSON-RPC 2.0 error.
#[derive(Debug, Deserialize)]
pub struct RpcError {
    pub message: String,
}

/// Health check result.
#[derive(Debug, Deserialize)]
pub struct HealthResult {
    pub healthy: bool,
    pub version: String,
    pub repo_count: u32,
    pub database_ok: bool,
    pub socket_path: String,
}

/// Connect to the daemon via IPC.
#[cfg(unix)]
pub async fn connect_to_daemon(paths: &AirlockPaths) -> Result<Stream> {
    let socket_name = paths.socket_name();
    let name = socket_name
        .to_fs_name::<GenericFilePath>()
        .context("Failed to create socket name")?;

    Stream::connect(name)
        .await
        .context("Failed to connect to daemon socket")
}

/// Connect to the daemon via IPC (Windows).
#[cfg(windows)]
pub async fn connect_to_daemon(paths: &AirlockPaths) -> Result<Stream> {
    let socket_name = paths.socket_name();
    let name = socket_name
        .to_ns_name::<GenericNamespaced>()
        .context("Failed to create named pipe name")?;

    Stream::connect(name)
        .await
        .context("Failed to connect to daemon named pipe")
}

/// Send a request to the daemon and get a response.
pub async fn send_request(paths: &AirlockPaths, request: &Request) -> Result<Response> {
    let stream = connect_to_daemon(paths).await?;
    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);

    // Send request
    let request_json = serde_json::to_string(request)?;
    debug!("Sending request: {}", request_json);
    writer.write_all(request_json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;

    // Read response
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    debug!("Received response: {}", line.trim());

    let response: Response = serde_json::from_str(&line)?;
    Ok(response)
}

/// Check if the daemon is running by trying to connect to it.
pub async fn is_daemon_running(paths: &AirlockPaths) -> bool {
    // First check if socket file exists (Unix only)
    #[cfg(unix)]
    if !paths.socket().exists() {
        return false;
    }

    // Try to connect and get health
    match send_request(paths, &Request::new("health")).await {
        Ok(response) => response.result.is_some(),
        Err(_) => false,
    }
}

/// Get the path to the daemon executable.
pub fn get_daemon_path() -> PathBuf {
    // Try to find airlockd relative to the current executable
    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let daemon_path = parent.join("airlockd");
            if daemon_path.exists() {
                return daemon_path;
            }
        }
    }

    // Fall back to system path
    PathBuf::from("airlockd")
}

/// Spawn the daemon as a background process with log redirection.
/// Returns the spawned child process and the logs directory path.
pub fn spawn_daemon_direct(
    paths: &AirlockPaths,
    daemon_path: &Path,
) -> Result<(std::process::Child, PathBuf)> {
    paths.ensure_dirs()?;
    let logs_dir = paths.root().join("logs");
    std::fs::create_dir_all(&logs_dir)?;

    let stdout_log = logs_dir.join("daemon.stdout.log");
    let stderr_log = logs_dir.join("daemon.stderr.log");

    let stdout_file =
        std::fs::File::create(&stdout_log).context("Failed to create stdout log file")?;
    let stderr_file =
        std::fs::File::create(&stderr_log).context("Failed to create stderr log file")?;

    let child = Command::new(daemon_path)
        .stdout(stdout_file)
        .stderr(stderr_file)
        .spawn()
        .with_context(|| format!("Failed to start daemon at: {}", daemon_path.display()))?;

    Ok((child, logs_dir))
}

/// Ensure the daemon is running, starting it if necessary.
/// Returns Ok(true) if daemon was started, Ok(false) if already running.
///
/// This function does NOT install service files. It only:
/// 1. Uses the service manager to start if service is already installed
/// 2. Falls back to spawning the daemon directly as a background process
///
/// Service installation requires explicit `airlock daemon install`.
///
/// Note: In debug builds, this function does NOT auto-start the daemon.
/// Developers must manually run `airlock daemon start` or `cargo run --bin airlockd`.
pub async fn ensure_daemon_running(paths: &AirlockPaths) -> Result<bool> {
    if is_daemon_running(paths).await {
        debug!("Daemon is already running");
        return Ok(false);
    }

    // In debug builds, don't auto-start the daemon - require manual start
    #[cfg(debug_assertions)]
    {
        debug!("Debug build: skipping daemon auto-start");
        Err(anyhow::anyhow!(
            "Daemon is not running.\n\
             In development, start it manually with:\n  \
             cargo run --bin airlockd\n  \
             or: airlock daemon start"
        ))
    }

    #[cfg(not(debug_assertions))]
    {
        info!("Daemon is not running, attempting to start...");

        let daemon_path = get_daemon_path();
        let service_manager = ServiceManager::new(daemon_path.clone())?;

        // Try to use the service manager first (macOS/Linux)
        if service_manager.is_installed() {
            debug!("Using service manager to start daemon");
            match service_manager.load() {
                Ok(()) => {
                    // Wait for daemon to be ready
                    for _ in 0..10 {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        if is_daemon_running(paths).await {
                            info!("Daemon started via service manager");
                            return Ok(true);
                        }
                    }
                    warn!("Service manager started daemon but it's not responding yet");
                }
                Err(e) => {
                    warn!("Service manager failed: {}. Trying direct start.", e);
                }
            }
        }

        // Direct start as fallback
        debug!("Starting daemon directly...");

        let (child, logs_dir) = spawn_daemon_direct(paths, &daemon_path)?;
        debug!("Daemon process started with PID: {}", child.id());

        // Wait for daemon to be ready
        for i in 0..20 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if is_daemon_running(paths).await {
                info!("Daemon started successfully (PID: {})", child.id());
                return Ok(true);
            }
            debug!("Waiting for daemon to start... attempt {}/20", i + 1);
        }

        // Daemon started but not responding - this is a warning, not an error
        // The hooks might still work once the daemon finishes initializing
        warn!(
            "Daemon process started but not responding yet. Check logs at: {}",
            logs_dir.display()
        );
        Ok(true)
    } // end #[cfg(not(debug_assertions))]
}
