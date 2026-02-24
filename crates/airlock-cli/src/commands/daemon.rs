//! `airlock daemon` command implementation.

use anyhow::{Context, Result};
use std::time::Duration;
use tracing::{debug, error, info, warn};

use airlock_core::{AirlockPaths, ServiceManager};

use super::ipc_client::{
    get_daemon_path, is_daemon_running, send_request, spawn_daemon_direct, HealthResult, Request,
};

/// Start the Airlock daemon.
pub async fn start() -> Result<()> {
    let paths = AirlockPaths::new().context("Failed to initialize Airlock paths")?;

    // Check if daemon is already running
    if is_daemon_running(&paths).await {
        println!("Daemon is already running.");
        return Ok(());
    }

    info!("Starting Airlock daemon...");

    let daemon_path = get_daemon_path();
    let service_manager = ServiceManager::new(daemon_path.clone())?;

    // Try to use the service manager first (macOS/Linux)
    if service_manager.is_installed() {
        info!("Using service manager to start daemon");
        match service_manager.load() {
            Ok(()) => {
                // Wait for daemon to be ready
                for _ in 0..10 {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    if is_daemon_running(&paths).await {
                        println!("Daemon started successfully.");
                        return Ok(());
                    }
                }
                println!("Daemon started, but taking longer than expected to respond.");
                println!("Check logs at: ~/.airlock/logs/");
                return Ok(());
            }
            Err(e) => {
                warn!(
                    "Service manager failed: {}. Falling back to direct start.",
                    e
                );
            }
        }
    }

    // Direct start as fallback (or if service not installed)
    info!("Starting daemon directly...");

    let (child, logs_dir) = spawn_daemon_direct(&paths, &daemon_path)?;
    info!("Daemon process started with PID: {}", child.id());

    // Wait for daemon to be ready
    for i in 0..20 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if is_daemon_running(&paths).await {
            println!("Daemon started successfully (PID: {}).", child.id());
            return Ok(());
        }
        debug!("Waiting for daemon to start... attempt {}/20", i + 1);
    }

    println!("Daemon process started, but not responding yet.");
    println!("Check logs at: {}", logs_dir.display());
    Ok(())
}

/// Stop the Airlock daemon.
pub async fn stop() -> Result<()> {
    let paths = AirlockPaths::new().context("Failed to initialize Airlock paths")?;

    // Check if daemon is running
    if !is_daemon_running(&paths).await {
        println!("Daemon is not running.");
        return Ok(());
    }

    info!("Stopping Airlock daemon...");

    // Send shutdown command via IPC
    let request = Request::new("shutdown");
    match send_request(&paths, &request).await {
        Ok(response) => {
            if let Some(error) = response.error {
                error!("Shutdown request failed: {}", error.message);
                return Err(anyhow::anyhow!("Failed to stop daemon: {}", error.message));
            }
            debug!("Shutdown acknowledged");
        }
        Err(e) => {
            warn!(
                "Failed to send shutdown command: {}. Daemon may have already stopped.",
                e
            );
        }
    }

    // Wait for daemon to stop
    for i in 0..20 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if !is_daemon_running(&paths).await {
            println!("Daemon stopped successfully.");
            return Ok(());
        }
        debug!("Waiting for daemon to stop... attempt {}/20", i + 1);
    }

    // Try to unload via service manager as last resort
    let daemon_path = get_daemon_path();
    if let Ok(service_manager) = ServiceManager::new(daemon_path) {
        if service_manager.is_installed() {
            debug!("Using service manager to unload daemon");
            let _ = service_manager.unload();
        }
    }

    // Check one more time
    tokio::time::sleep(Duration::from_millis(500)).await;
    if !is_daemon_running(&paths).await {
        println!("Daemon stopped successfully.");
    } else {
        println!("Daemon may still be running. Check manually.");
    }

    Ok(())
}

/// Restart the Airlock daemon.
pub async fn restart() -> Result<()> {
    info!("Restarting Airlock daemon...");

    stop().await?;

    // Give it a moment before starting again
    tokio::time::sleep(Duration::from_millis(500)).await;

    start().await?;

    Ok(())
}

/// Check the status of the Airlock daemon.
pub async fn status() -> Result<()> {
    let paths = AirlockPaths::new().context("Failed to initialize Airlock paths")?;

    println!("Daemon Status");
    println!("─────────────");
    println!();

    // Check socket file (Unix only)
    #[cfg(unix)]
    {
        let socket_path = paths.socket();
        println!("Socket: {}", socket_path.display());
        if !socket_path.exists() {
            println!("Status: Not running (socket does not exist)");
            println!();
            println!("Start the daemon with: airlock daemon start");
            return Ok(());
        }
    }

    // Try to get health from daemon
    match send_request(&paths, &Request::new("health")).await {
        Ok(response) => {
            if let Some(result) = response.result {
                let health: HealthResult =
                    serde_json::from_value(result).context("Failed to parse health result")?;

                println!(
                    "Status: {} (healthy: {})",
                    if health.healthy {
                        "Running"
                    } else {
                        "Unhealthy"
                    },
                    health.healthy
                );
                println!("Version: {}", health.version);
                println!("Repositories: {}", health.repo_count);
                println!(
                    "Database: {}",
                    if health.database_ok { "OK" } else { "Error" }
                );
                println!("Socket: {}", health.socket_path);
            } else if let Some(error) = response.error {
                println!("Status: Error");
                println!("Error: {}", error.message);
            }
        }
        Err(e) => {
            println!("Status: Not responding");
            println!("Error: {}", e);
            println!();
            println!("The socket file exists but the daemon is not responding.");
            println!("Try restarting: airlock daemon restart");
        }
    }

    println!();

    // Show service installation status
    let daemon_path = get_daemon_path();
    if let Ok(service_manager) = ServiceManager::new(daemon_path) {
        if service_manager.is_installed() {
            println!("Service: Installed");
            #[cfg(target_os = "macos")]
            println!(
                "  Plist: {}",
                service_manager.launchd_plist_path().display()
            );
            #[cfg(target_os = "linux")]
            println!("  Unit: {}", service_manager.systemd_unit_path().display());
        } else {
            println!("Service: Not installed");
            println!("  To enable auto-start, run: airlock daemon install");
        }
    }

    Ok(())
}

/// Install the daemon as a system service.
pub async fn install() -> Result<()> {
    let paths = AirlockPaths::new().context("Failed to initialize Airlock paths")?;
    let daemon_path = get_daemon_path();

    // Verify daemon executable exists
    if !daemon_path.exists() && daemon_path.as_path() != std::path::Path::new("airlockd") {
        return Err(anyhow::anyhow!(
            "Daemon executable not found at: {}\n\
             Please ensure airlockd is installed.",
            daemon_path.display()
        ));
    }

    let service_manager = ServiceManager::new(daemon_path)?;

    // Install service files
    let path = service_manager.install()?;
    println!("Service installed at: {}", path.display());

    // Ensure logs directory exists
    let logs_dir = paths.root().join("logs");
    std::fs::create_dir_all(&logs_dir)?;

    // Load the service so it starts now and on future logins
    match service_manager.load() {
        Ok(()) => {
            info!("Service loaded successfully");
        }
        Err(e) => {
            warn!(
                "Failed to load service: {}. You can start it manually with: airlock daemon start",
                e
            );
        }
    }

    Ok(())
}

/// Uninstall the daemon from system services.
pub async fn uninstall() -> Result<()> {
    let daemon_path = get_daemon_path();
    let service_manager = ServiceManager::new(daemon_path)?;

    // Stop the service first
    if service_manager.is_installed() {
        info!("Stopping service before uninstalling...");
        let _ = service_manager.unload();
    }

    // Uninstall service files
    service_manager.uninstall()?;
    println!("Service uninstalled.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let request = Request::new("health");
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"method\":\"health\""));
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
    }
}
