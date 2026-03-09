//! `airlock init` command implementation.

use anyhow::{Context, Result};
use dialoguer::Confirm;
use std::env;
use std::fs;
use std::path::Path;
use tracing::warn;

use airlock_core::{config::GlobalConfig, init, AgentGlobalConfig, AirlockPaths, Database};

use super::init_wizard;

/// Run the init command to set up Airlock in the current repository.
pub async fn run() -> Result<()> {
    let paths = AirlockPaths::new().context("Failed to initialize Airlock paths")?;
    let current_dir = env::current_dir().context("Failed to get current directory")?;

    // Check if the repo is already enrolled and handle reinstall confirmation
    check_and_handle_existing(&current_dir, &paths)?;

    let first_time = !paths.global_config().exists();

    // Run the interactive wizard
    let wizard_result = init_wizard::run_wizard(first_time)?;

    // If first-time, write global config with chosen agent adapter
    if first_time {
        if let Some(adapter) = &wizard_result.agent_adapter {
            write_global_config(&paths, adapter)?;
        }
    }

    // Run the init logic
    run_with_paths(&current_dir, &paths)?;

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

/// Check if the repository is already enrolled and prompt for reinstall.
///
/// If the repo is already initialized, asks the user whether to reinstall.
/// On confirmation, ejects first so init can proceed cleanly.
fn check_and_handle_existing(working_dir: &Path, paths: &AirlockPaths) -> Result<()> {
    paths
        .ensure_dirs()
        .context("Failed to create Airlock directories")?;
    let db = Database::open(&paths.database()).context("Failed to open Airlock database")?;

    let existing = init::check_existing_enrollment(working_dir, &db)?;
    let Some(enrollment) = existing else {
        return Ok(());
    };

    println!(
        "This repository is already set up with Airlock (upstream: {}).",
        enrollment.upstream_url
    );

    let reinstall = Confirm::new()
        .with_prompt("Would you like to reinstall Airlock?")
        .default(false)
        .interact()?;

    if !reinstall {
        anyhow::bail!("Init cancelled.");
    }

    println!();
    println!("Ejecting existing Airlock setup...");
    init::eject_repo(working_dir, paths, &db)?;
    println!("Done. Re-initializing...");
    println!();

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

/// Internal implementation that accepts paths for testability.
pub(crate) fn run_with_paths(working_dir: &Path, paths: &AirlockPaths) -> Result<()> {
    use indicatif::{ProgressBar, ProgressStyle};

    paths
        .ensure_dirs()
        .context("Failed to create Airlock directories")?;
    let db = Database::open(&paths.database()).context("Failed to open Airlock database")?;

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner} {msg}")
            .unwrap(),
    );
    spinner.set_message("Setting up Airlock proxy repository...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    // Suppress info/debug logs while spinner is active to avoid corrupting the output.
    // Warnings and errors still get through.
    let quiet_subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new("warn"))
        .with_target(false)
        .with_writer(std::io::stderr)
        .finish();
    let result = tracing::subscriber::with_default(quiet_subscriber, || {
        init::init_repo(working_dir, paths, &db)
    });
    spinner.finish_and_clear();
    let outcome = result?;

    // Success!
    println!("✓ Airlock initialized successfully!");
    println!();
    if outcome.config_created {
        println!("Created .airlock/workflows/main.yml with default workflow configuration.");
        println!("Edit this file to customize your pipeline steps.");
        println!();
    }
    println!("Your remotes have been reconfigured:");
    println!("  origin         → local Airlock gate (pushes are intercepted)");
    println!("  bypass-airlock → {} (escape hatch)", outcome.upstream_url);
    println!();
    println!("Push as normal with `git push origin <branch>`.");
    println!("Use `git push bypass-airlock <branch>` to bypass Airlock.");

    Ok(())
}

#[cfg(test)]
#[path = "init_tests.rs"]
mod tests;
