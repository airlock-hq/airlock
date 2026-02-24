//! `airlock nuke` command implementation.
//!
//! Completely removes all Airlock data (~/.airlock/).

use anyhow::{Context, Result};
use std::io::{self, Write};
use tracing::info;

use airlock_core::AirlockPaths;

use super::ipc_client::is_daemon_running;

/// Run the nuke command.
///
/// This will:
/// 1. Show a warning and ask for confirmation (unless --force)
/// 2. Stop the daemon if running
/// 3. Delete ~/.airlock/ directory
/// 4. Restart the daemon if it was running before
pub async fn run(force: bool) -> Result<()> {
    let paths = AirlockPaths::new().context("Failed to initialize Airlock paths")?;
    let root = paths.root().clone();

    // Check if there's anything to delete
    if !root.exists() {
        println!("Nothing to do: {} does not exist.", root.display());
        return Ok(());
    }

    // Confirm unless --force
    if !force {
        println!("⚠️  WARNING: This will permanently delete all Airlock data:");
        println!("   {}", root.display());
        println!();
        println!("   This includes:");
        println!("   - All enrolled repositories (gate repos)");
        println!("   - Run history and artifacts");
        println!("   - Database and configuration");
        println!();
        print!("Type 'yes' to confirm: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if input.trim() != "yes" {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Check if daemon is running before stopping
    let daemon_was_running = is_daemon_running(&paths).await;

    // Stop daemon if running
    if daemon_was_running {
        info!("Stopping daemon before nuke...");
        println!("Stopping daemon...");
        super::daemon::stop().await?;
    }

    // Delete the directory
    info!("Deleting Airlock data directory: {}", root.display());
    println!("Deleting {}...", root.display());

    std::fs::remove_dir_all(&root).with_context(|| {
        format!(
            "Failed to delete Airlock data directory: {}",
            root.display()
        )
    })?;

    println!("✓ Airlock data deleted.");

    // Restart daemon if it was running before
    if daemon_was_running {
        info!("Restarting daemon after nuke...");
        println!("Restarting daemon...");
        super::daemon::start().await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_nuke_nonexistent_directory() {
        // Create a temp dir for AIRLOCK_HOME that doesn't exist
        let temp = TempDir::new().unwrap();
        let nonexistent = temp.path().join("nonexistent");

        std::env::set_var("AIRLOCK_HOME", &nonexistent);

        // Should succeed and print "nothing to do"
        let result = run(true).await;
        assert!(result.is_ok());

        std::env::remove_var("AIRLOCK_HOME");
    }

    #[tokio::test]
    async fn test_nuke_deletes_directory() {
        let temp = TempDir::new().unwrap();
        let airlock_home = temp.path().join("airlock-test");

        // Create the directory with some files
        fs::create_dir_all(&airlock_home).unwrap();
        fs::write(airlock_home.join("state.sqlite"), "test").unwrap();
        fs::create_dir_all(airlock_home.join("repos")).unwrap();
        fs::write(airlock_home.join("repos/test.git"), "test").unwrap();

        assert!(airlock_home.exists());

        std::env::set_var("AIRLOCK_HOME", &airlock_home);

        // Run nuke with force flag
        let result = run(true).await;
        assert!(result.is_ok());

        // Directory should be gone
        assert!(!airlock_home.exists());

        std::env::remove_var("AIRLOCK_HOME");
    }
}
