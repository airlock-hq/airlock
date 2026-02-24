//! Airlock Daemon
//!
//! Background service that manages repos, runs pipelines, and handles IPC.

use anyhow::Result;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

mod cleanup;
mod handlers;
mod ipc;
mod pipeline;
mod push_coalescer;
mod run_queue;
mod server;
mod stage_loader;
mod sync;

use server::Server;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .with_target(true)
        .init();

    info!("Starting Airlock daemon...");

    // Ensure Airlock directories exist
    let paths = airlock_core::AirlockPaths::new()?;
    paths.ensure_dirs()?;

    // Run artifact cleanup on startup
    let cleanup_result = cleanup::cleanup_old_artifacts(&paths);
    if cleanup_result.deleted_count > 0 {
        info!(
            "Startup cleanup: deleted {} old artifact directories, freed {} bytes",
            cleanup_result.deleted_count, cleanup_result.bytes_freed
        );
    }

    // Create and run the server
    let server = Server::new(paths)?;

    // Handle shutdown gracefully
    tokio::select! {
        result = server.run() => {
            if let Err(e) = result {
                error!("Server error: {}", e);
                return Err(e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received shutdown signal");
        }
    }

    info!("Airlock daemon stopped");
    Ok(())
}
