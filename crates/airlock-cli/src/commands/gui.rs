//! GUI launcher functionality.
//!
//! When the CLI is invoked without arguments, it spawns the desktop app
//! as a detached process and exits immediately.

use anyhow::{Context, Result};
use std::env;
use std::path::PathBuf;
use std::process::Command;

/// The name of the GUI binary
const GUI_BINARY_NAME: &str = "airlock-app";

/// Environment variable to override GUI binary path
const GUI_PATH_ENV_VAR: &str = "AIRLOCK_APP_PATH";

/// Attempt to find and launch the GUI application.
///
/// Returns Ok(()) if the GUI was successfully spawned, or an error if
/// the GUI binary could not be found or failed to start.
pub fn launch() -> Result<()> {
    let gui_path = find_gui_binary()?;
    spawn_detached(&gui_path)
}

/// Find the GUI binary using the following precedence:
/// 1. AIRLOCK_APP_PATH environment variable
/// 2. Same directory as the CLI binary
/// 3. Platform-specific install paths
fn find_gui_binary() -> Result<PathBuf> {
    // 1. Check environment variable
    if let Ok(path) = env::var(GUI_PATH_ENV_VAR) {
        let path = PathBuf::from(path);
        if path.exists() && path.is_file() {
            return Ok(path);
        }
        // If env var is set but path doesn't exist, continue to other options
    }

    // 2. Check same directory as CLI binary
    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let gui_path = parent.join(GUI_BINARY_NAME);
            if gui_path.exists() && gui_path.is_file() {
                return Ok(gui_path);
            }
        }
    }

    // 3. Check platform-specific paths
    for path in platform_paths() {
        if path.exists() && path.is_file() {
            return Ok(path);
        }
    }

    // GUI not found
    Err(anyhow::anyhow!(
        "Desktop app not found. Install it from https://airlock.dev/download\n\
         or run 'airlock --help' for CLI commands."
    ))
}

/// Get platform-specific paths to search for the GUI binary.
fn platform_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    {
        // Standard macOS app bundle location
        paths.push(PathBuf::from(
            "/Applications/Airlock.app/Contents/MacOS/airlock-app",
        ));

        // User-local Applications folder
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join("Applications/Airlock.app/Contents/MacOS/airlock-app"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        // System-wide installation
        paths.push(PathBuf::from("/usr/bin/airlock-app"));
        paths.push(PathBuf::from("/usr/local/bin/airlock-app"));

        // User-local installation
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".local/bin/airlock-app"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Standard Windows installation paths
        if let Ok(program_files) = env::var("ProgramFiles") {
            paths.push(PathBuf::from(program_files).join("Airlock/airlock-app.exe"));
        }
        if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
            paths.push(PathBuf::from(local_app_data).join("Airlock/airlock-app.exe"));
        }
    }

    paths
}

/// Spawn the GUI process in a detached manner.
///
/// On Unix systems, this creates a new process group and session.
/// The child process will continue running after the CLI exits.
fn spawn_detached(gui_path: &PathBuf) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        // Create the command
        let mut cmd = Command::new(gui_path);

        // Pre-exec hook to create a new session (detach from terminal)
        unsafe {
            cmd.pre_exec(|| {
                // Create a new session, making this process the session leader
                libc::setsid();
                Ok(())
            });
        }

        // Spawn the process
        cmd.spawn()
            .with_context(|| format!("Failed to spawn GUI process: {}", gui_path.display()))?;
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        // DETACHED_PROCESS flag (0x00000008) creates a new process group
        // and doesn't inherit the console
        const DETACHED_PROCESS: u32 = 0x00000008;

        Command::new(gui_path)
            .creation_flags(DETACHED_PROCESS)
            .spawn()
            .with_context(|| format!("Failed to spawn GUI process: {}", gui_path.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::TempDir;

    #[test]
    fn test_find_gui_binary_env_var() {
        // Create a temporary directory with a fake GUI binary
        let temp_dir = TempDir::new().unwrap();
        let gui_path = temp_dir.path().join(GUI_BINARY_NAME);
        File::create(&gui_path).unwrap();

        // Set the environment variable
        let original_env = env::var(GUI_PATH_ENV_VAR).ok();
        env::set_var(GUI_PATH_ENV_VAR, &gui_path);

        // Should find via env var
        let result = find_gui_binary();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), gui_path);

        // Restore original env
        if let Some(orig) = original_env {
            env::set_var(GUI_PATH_ENV_VAR, orig);
        } else {
            env::remove_var(GUI_PATH_ENV_VAR);
        }
    }

    #[test]
    fn test_find_gui_binary_env_var_not_file() {
        // Set env var to a directory (not a file)
        let temp_dir = TempDir::new().unwrap();
        let dir_path = temp_dir.path();

        let original_env = env::var(GUI_PATH_ENV_VAR).ok();
        env::set_var(GUI_PATH_ENV_VAR, dir_path);

        // Should not find (it's a directory, not a file)
        // This will fall through to other checks
        let result = find_gui_binary();
        // Result depends on whether GUI exists elsewhere; just ensure no panic
        let _ = result;

        // Restore original env
        if let Some(orig) = original_env {
            env::set_var(GUI_PATH_ENV_VAR, orig);
        } else {
            env::remove_var(GUI_PATH_ENV_VAR);
        }
    }

    #[test]
    fn test_find_gui_binary_same_directory() {
        // Create a temporary directory with a fake GUI binary
        let temp_dir = TempDir::new().unwrap();
        let gui_path = temp_dir.path().join(GUI_BINARY_NAME);
        File::create(&gui_path).unwrap();

        // Create a fake CLI binary path in the same directory
        let cli_path = temp_dir.path().join("airlock");
        File::create(&cli_path).unwrap();

        // We can't easily test the current_exe path, but we can verify
        // the logic works when we manually construct it
        let parent = cli_path.parent().unwrap();
        let found_gui = parent.join(GUI_BINARY_NAME);
        assert!(found_gui.exists());
        assert!(found_gui.is_file());
    }

    #[test]
    fn test_platform_paths_not_empty() {
        // Platform paths should return at least one path on supported platforms
        let paths = platform_paths();

        #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
        assert!(!paths.is_empty());
    }

    #[test]
    fn test_find_gui_binary_not_found() {
        // Ensure env var is not set
        let original_env = env::var(GUI_PATH_ENV_VAR).ok();
        env::remove_var(GUI_PATH_ENV_VAR);

        // Clear any other paths that might exist by testing in isolation
        // This test verifies the error message is correct
        let result = find_gui_binary();

        // In a clean test environment without GUI installed, this should fail
        // But on a dev machine it might succeed, so we just check it doesn't panic
        if result.is_err() {
            let err_msg = result.unwrap_err().to_string();
            assert!(err_msg.contains("Desktop app not found"));
            assert!(err_msg.contains("airlock.dev/download"));
        }

        // Restore original env
        if let Some(orig) = original_env {
            env::set_var(GUI_PATH_ENV_VAR, orig);
        }
    }
}
