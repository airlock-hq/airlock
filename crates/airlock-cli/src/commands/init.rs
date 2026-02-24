//! `airlock init` command implementation.

use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::Path;
use tracing::warn;

use airlock_core::{config::GlobalConfig, init, AgentGlobalConfig, AirlockPaths, Database};

use super::init_wizard;

/// Run the init command to set up Airlock in the current repository.
pub async fn run() -> Result<()> {
    let paths = AirlockPaths::new().context("Failed to initialize Airlock paths")?;
    let first_time = !paths.global_config().exists();

    // Run the interactive wizard
    let wizard_result = init_wizard::run_wizard(first_time)?;

    // If first-time, write global config with chosen agent adapter
    if first_time {
        if let Some(adapter) = &wizard_result.agent_adapter {
            write_global_config(&paths, adapter)?;
        }
    }

    // Run the existing init logic (unchanged)
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    run_with_paths(&current_dir, &paths)?;

    // If user opted out of approval, patch the workflow YAML
    if !wizard_result.require_approval {
        patch_workflow_approval(&current_dir)?;
    }

    // Ensure the daemon is running (fallback auto-start)
    match super::ipc_client::ensure_daemon_running(&paths).await {
        Ok(started) => {
            if started {
                println!("Daemon started automatically.");
            }
        }
        Err(e) => {
            warn!("Could not start daemon automatically: {}", e);
            println!();
            println!("Note: The daemon is not running. Start it with:");
            println!("  airlock daemon start");
        }
    }

    Ok(())
}

/// Write global config with the chosen agent adapter.
fn write_global_config(paths: &AirlockPaths, adapter: &str) -> Result<()> {
    paths
        .ensure_dirs()
        .context("Failed to create Airlock directories")?;

    let config = GlobalConfig {
        agent: AgentGlobalConfig {
            adapter: adapter.to_string(),
            ..Default::default()
        },
        ..Default::default()
    };

    let yaml = serde_yaml::to_string(&config).context("Failed to serialize global config")?;
    fs::write(paths.global_config(), yaml).context("Failed to write global config")?;

    println!(
        "Created global config at {}",
        paths.global_config().display()
    );

    Ok(())
}

/// Patch the default workflow to disable the require-approval gate.
fn patch_workflow_approval(working_dir: &Path) -> Result<()> {
    let workflow_path = working_dir
        .join(init::REPO_CONFIG_PATH)
        .join(init::DEFAULT_WORKFLOW_FILENAME);

    if !workflow_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&workflow_path)?;
    let patched = content.replace("require-approval: true", "require-approval: false");
    fs::write(&workflow_path, patched)?;

    Ok(())
}

/// Internal implementation that accepts paths for testability.
pub(crate) fn run_with_paths(working_dir: &Path, paths: &AirlockPaths) -> Result<()> {
    paths
        .ensure_dirs()
        .context("Failed to create Airlock directories")?;
    let db = Database::open(&paths.database()).context("Failed to open Airlock database")?;

    let outcome = init::init_repo(working_dir, paths, &db)?;

    // Success!
    println!("✓ Airlock initialized successfully!");
    println!();
    if outcome.config_created {
        println!("Created .airlock/workflows/main.yml with default workflow configuration.");
        println!("Edit this file to customize your pipeline steps.");
        println!();
    }
    println!("Your remotes have been reconfigured:");
    println!("  origin   → local Airlock gate (pushes are intercepted)");
    println!("  upstream → {} (escape hatch)", outcome.upstream_url);
    println!();
    println!("Push as normal with `git push origin <branch>`.");
    println!("Use `git push upstream <branch>` to bypass Airlock.");

    Ok(())
}

#[cfg(test)]
#[path = "init_tests.rs"]
mod tests;
