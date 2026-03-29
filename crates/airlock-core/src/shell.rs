//! User shell and PATH resolution utilities.
//!
//! The daemon process (launched by launchd/systemd) typically inherits a
//! minimal PATH that lacks user-installed tools like `claude`, `node`,
//! `cargo`, etc.  This module resolves the user's real PATH by spawning
//! their login shell, and caches the result for the process lifetime.
//!
//! Both the pipeline executor (for step scripts) and the agent adapters
//! (for CLI availability checks and subprocess spawning) use this module.

use std::sync::OnceLock;
use tracing::{debug, info, warn};

/// Cached user login shell path, resolved once at first use.
static USER_LOGIN_SHELL: OnceLock<String> = OnceLock::new();

/// Cached user PATH from login shell, resolved once at first use.
static USER_PATH: OnceLock<String> = OnceLock::new();

/// Detect the user's login shell.
///
/// The daemon process (launched by launchd/systemd) typically does not have
/// `$SHELL` set. This function checks, in order:
/// 1. `$SHELL` environment variable
/// 2. System user database (macOS `dscl`, Linux `getent`)
/// 3. Falls back to `bash`
pub fn get_user_login_shell() -> &'static str {
    USER_LOGIN_SHELL.get_or_init(|| {
        // 1. Try $SHELL env var
        if let Ok(shell) = std::env::var("SHELL") {
            if !shell.is_empty() {
                debug!("User login shell from $SHELL: {}", shell);
                return shell;
            }
        }

        // 2. Query the OS user database
        if let Some(shell) = detect_shell_from_system() {
            debug!("User login shell from system: {}", shell);
            return shell;
        }

        debug!("Could not detect user login shell, falling back to bash");
        "bash".to_string()
    })
}

/// Query the OS user database for the user's configured login shell.
fn detect_shell_from_system() -> Option<String> {
    // Determine the username from $USER or `whoami`
    let username = std::env::var("USER").ok().or_else(|| {
        std::process::Command::new("whoami")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    })?;

    #[cfg(target_os = "macos")]
    {
        // dscl output format: "UserShell: /bin/zsh"
        let output = std::process::Command::new("dscl")
            .args([".", "-read", &format!("/Users/{}", username), "UserShell"])
            .output()
            .ok()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(shell) = stdout.trim().strip_prefix("UserShell:") {
                let shell = shell.trim();
                if !shell.is_empty() {
                    return Some(shell.to_string());
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // getent output format: "username:x:uid:gid:info:home:shell"
        let output = std::process::Command::new("getent")
            .args(["passwd", &username])
            .output()
            .ok()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(shell) = stdout.trim().rsplit(':').next() {
                if !shell.is_empty() {
                    return Some(shell.to_string());
                }
            }
        }
    }

    None
}

/// Check whether a shell supports the `-i` (interactive) flag with `-c`.
///
/// Shells like `bash` and `zsh` keep some PATH/env setup in their
/// interactive-only config files (`~/.bashrc`, `~/.zshrc`). Without `-i`,
/// `shell -l -c …` won't source those files, so tools installed via nvm,
/// fnm, rustup, etc. may be missing from PATH.
///
/// We allowlist known shells rather than passing `-i` unconditionally,
/// because other shells (fish, nu, etc.) may not accept the same flags.
pub fn shell_supports_interactive(shell: &str) -> bool {
    let basename = shell.rsplit('/').next().unwrap_or(shell);
    matches!(basename, "bash" | "zsh")
}

/// Resolve the user's full PATH by spawning their login shell.
///
/// Captures the PATH as the user would see it in a terminal session,
/// including additions from shell profiles (Homebrew, nvm, rustup, etc.).
/// The result is cached for the process lifetime.
pub fn resolve_user_path() -> &'static str {
    USER_PATH.get_or_init(|| {
        let shell = get_user_login_shell();

        // Use -i (interactive) for shells that support it so that
        // interactive-only config files (~/.bashrc, ~/.zshrc) are sourced.
        let args: Vec<&str> = if shell_supports_interactive(shell) {
            debug!("Resolving user PATH via '{} -l -i -c echo $PATH'", shell);
            vec!["-l", "-i", "-c", "echo $PATH"]
        } else {
            debug!("Resolving user PATH via '{} -l -c echo $PATH'", shell);
            vec!["-l", "-c", "echo $PATH"]
        };

        let result = std::process::Command::new(shell).args(&args).output();

        match result {
            Ok(output) if output.status.success() => {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    info!(
                        "Resolved user PATH via login shell ({} entries)",
                        path.split(':').count()
                    );
                    return path;
                }
                warn!("Login shell returned empty PATH");
            }
            Ok(output) => {
                warn!("Login shell '{}' exited with {}", shell, output.status);
            }
            Err(e) => {
                warn!("Failed to spawn login shell '{}': {}", shell, e);
            }
        }

        std::env::var("PATH")
            .unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin".to_string())
    })
}
