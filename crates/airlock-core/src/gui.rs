//! GUI binary discovery and launching.
//!
//! Shared logic for finding and spawning the desktop app binary.
//! Used by the CLI (when invoked without arguments) and the daemon
//! (to auto-launch the app on git push).

use crate::error::{AirlockError, Result};
use std::env;
use std::path::PathBuf;
use std::process::Command;

/// The name of the GUI binary.
pub const GUI_BINARY_NAME: &str = "airlock-app";

/// Environment variable to override GUI binary path.
pub const GUI_PATH_ENV_VAR: &str = "AIRLOCK_APP_PATH";

/// Find the GUI binary using the following precedence:
/// 1. AIRLOCK_APP_PATH environment variable
/// 2. Same directory as the current binary
/// 3. Platform-specific install paths
pub fn find_gui_binary() -> Result<PathBuf> {
    // 1. Check environment variable
    if let Ok(path) = env::var(GUI_PATH_ENV_VAR) {
        let path = PathBuf::from(path);
        if path.exists() && path.is_file() {
            return Ok(path);
        }
    }

    // 2. Check same directory as current binary
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

    Err(AirlockError::Other(
        "Desktop app not found. Install it from https://airlock.dev/download\n\
         or run 'airlock --help' for CLI commands."
            .into(),
    ))
}

/// Get platform-specific paths to search for the GUI binary.
fn platform_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from(
            "/Applications/Airlock.app/Contents/MacOS/airlock-app",
        ));

        if let Some(home) = dirs::home_dir() {
            paths.push(home.join("Applications/Airlock.app/Contents/MacOS/airlock-app"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        paths.push(PathBuf::from("/usr/bin/airlock-app"));
        paths.push(PathBuf::from("/usr/local/bin/airlock-app"));

        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".local/bin/airlock-app"));
        }
    }

    #[cfg(target_os = "windows")]
    {
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
/// The child process will continue running after the caller exits.
pub fn spawn_detached(gui_path: &PathBuf) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        let mut cmd = Command::new(gui_path);

        // SAFETY: `setsid()` is called in the child process after `fork()` and
        // before `exec()`.  It only affects the child's session/process-group and
        // cannot fail in a way that corrupts the parent.  The closure captures no
        // shared state, so there are no data-race concerns.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }

        cmd.spawn().map_err(|e| {
            AirlockError::Other(format!(
                "Failed to spawn GUI process: {}: {e}",
                gui_path.display()
            ))
        })?;
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        const DETACHED_PROCESS: u32 = 0x00000008;

        Command::new(gui_path)
            .creation_flags(DETACHED_PROCESS)
            .spawn()
            .map_err(|e| {
                AirlockError::Other(format!(
                    "Failed to spawn GUI process: {}: {e}",
                    gui_path.display()
                ))
            })?;
    }

    Ok(())
}
