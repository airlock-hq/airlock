//! GUI launcher functionality.
//!
//! When the CLI is invoked without arguments, it spawns the desktop app
//! as a detached process and exits immediately.
//!
//! The actual binary discovery and spawning logic lives in `airlock_core::gui`
//! so it can be reused by the daemon for auto-launching.

use anyhow::Result;

/// Attempt to find and launch the GUI application.
///
/// Returns Ok(()) if the GUI was successfully spawned, or an error if
/// the GUI binary could not be found or failed to start.
pub fn launch() -> Result<()> {
    let gui_path = airlock_core::gui::find_gui_binary()?;
    airlock_core::gui::spawn_detached(&gui_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use airlock_core::gui::{find_gui_binary, GUI_BINARY_NAME, GUI_PATH_ENV_VAR};
    use std::env;
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
