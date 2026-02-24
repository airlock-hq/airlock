//! Service management for the Airlock daemon.
//!
//! This module provides utilities for installing and managing the daemon as a
//! system service (launchd on macOS, systemd on Linux).

use crate::error::{AirlockError, Result};
use crate::paths::AirlockPaths;
use std::path::PathBuf;

/// Launchd plist template for macOS.
#[cfg(target_os = "macos")]
const LAUNCHD_PLIST_TEMPLATE: &str = include_str!("../resources/dev.airlock.daemon.plist");

/// Systemd unit template for Linux.
#[cfg(target_os = "linux")]
const SYSTEMD_UNIT_TEMPLATE: &str = include_str!("../resources/airlockd.service");

/// Service manager for the Airlock daemon.
#[derive(Debug, Clone)]
pub struct ServiceManager {
    /// Path to the daemon executable.
    daemon_path: PathBuf,
    /// User's home directory.
    home_dir: PathBuf,
    /// Airlock paths.
    paths: AirlockPaths,
}

impl ServiceManager {
    /// Create a new service manager.
    ///
    /// # Arguments
    /// * `daemon_path` - Path to the airlockd executable
    pub fn new(daemon_path: PathBuf) -> Result<Self> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| AirlockError::Filesystem("Could not determine home directory".into()))?;

        let paths = AirlockPaths::new()?;

        Ok(Self {
            daemon_path,
            home_dir,
            paths,
        })
    }

    /// Create a new service manager with a custom home directory.
    /// Useful for testing to avoid depending on the real home directory.
    pub fn with_home_dir(daemon_path: PathBuf, home_dir: PathBuf) -> Result<Self> {
        let paths = AirlockPaths::with_root(home_dir.join(".airlock"));

        Ok(Self {
            daemon_path,
            home_dir,
            paths,
        })
    }

    /// Get the path to the launchd plist file (macOS only).
    #[cfg(target_os = "macos")]
    pub fn launchd_plist_path(&self) -> PathBuf {
        self.home_dir
            .join("Library/LaunchAgents/dev.airlock.daemon.plist")
    }

    /// Get the path to the systemd unit file (Linux only).
    #[cfg(target_os = "linux")]
    pub fn systemd_unit_path(&self) -> PathBuf {
        self.home_dir.join(".config/systemd/user/airlockd.service")
    }

    /// Generate the launchd plist content with actual paths.
    #[cfg(target_os = "macos")]
    fn generate_launchd_plist(&self) -> String {
        LAUNCHD_PLIST_TEMPLATE
            .replace("{{DAEMON_PATH}}", &self.daemon_path.to_string_lossy())
            .replace("{{HOME}}", &self.home_dir.to_string_lossy())
    }

    /// Generate the systemd unit content with actual paths.
    #[cfg(target_os = "linux")]
    fn generate_systemd_unit(&self) -> String {
        SYSTEMD_UNIT_TEMPLATE.replace("{{DAEMON_PATH}}", &self.daemon_path.to_string_lossy())
    }

    /// Install the service files for the current platform.
    pub fn install(&self) -> Result<PathBuf> {
        // Ensure logs directory exists
        let logs_dir = self.paths.root().join("logs");
        std::fs::create_dir_all(&logs_dir)?;

        #[cfg(target_os = "macos")]
        {
            self.install_launchd()
        }

        #[cfg(target_os = "linux")]
        {
            self.install_systemd()
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Err(AirlockError::Unsupported(
                "Service installation is only supported on macOS and Linux".into(),
            ))
        }
    }

    /// Install the launchd plist (macOS).
    #[cfg(target_os = "macos")]
    fn install_launchd(&self) -> Result<PathBuf> {
        let plist_path = self.launchd_plist_path();

        // Ensure parent directory exists
        if let Some(parent) = plist_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Write the plist file
        let content = self.generate_launchd_plist();
        std::fs::write(&plist_path, content)?;

        tracing::info!("Installed launchd plist at: {}", plist_path.display());
        Ok(plist_path)
    }

    /// Install the systemd unit (Linux).
    #[cfg(target_os = "linux")]
    fn install_systemd(&self) -> Result<PathBuf> {
        let unit_path = self.systemd_unit_path();

        // Ensure parent directory exists
        if let Some(parent) = unit_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Write the unit file
        let content = self.generate_systemd_unit();
        std::fs::write(&unit_path, content)?;

        tracing::info!("Installed systemd unit at: {}", unit_path.display());
        Ok(unit_path)
    }

    /// Uninstall the service files for the current platform.
    pub fn uninstall(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            self.uninstall_launchd()
        }

        #[cfg(target_os = "linux")]
        {
            self.uninstall_systemd()
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Err(AirlockError::Unsupported(
                "Service uninstallation is only supported on macOS and Linux".into(),
            ))
        }
    }

    /// Uninstall the launchd plist (macOS).
    #[cfg(target_os = "macos")]
    fn uninstall_launchd(&self) -> Result<()> {
        let plist_path = self.launchd_plist_path();
        if plist_path.exists() {
            std::fs::remove_file(&plist_path)?;
            tracing::info!("Removed launchd plist: {}", plist_path.display());
        }
        Ok(())
    }

    /// Uninstall the systemd unit (Linux).
    #[cfg(target_os = "linux")]
    fn uninstall_systemd(&self) -> Result<()> {
        let unit_path = self.systemd_unit_path();
        if unit_path.exists() {
            std::fs::remove_file(&unit_path)?;
            tracing::info!("Removed systemd unit: {}", unit_path.display());
        }
        Ok(())
    }

    /// Check if the service files are installed.
    pub fn is_installed(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.launchd_plist_path().exists()
        }

        #[cfg(target_os = "linux")]
        {
            self.systemd_unit_path().exists()
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            false
        }
    }

    /// Load the service (start it via the system service manager).
    #[cfg(target_os = "macos")]
    pub fn load(&self) -> Result<()> {
        let plist_path = self.launchd_plist_path();
        if !plist_path.exists() {
            return Err(AirlockError::ServiceNotInstalled);
        }

        // SAFETY: getuid() is always safe to call on Unix
        let uid = unsafe { libc::getuid() };
        let domain_target = format!("gui/{uid}");
        let service_target = format!("gui/{uid}/dev.airlock.daemon");

        // Clear any stale service registration before bootstrapping.
        // This handles upgrades where the previous unload may have failed.
        let _ = std::process::Command::new("launchctl")
            .args(["bootout", &service_target])
            .output();

        let output = std::process::Command::new("launchctl")
            .args(["bootstrap", &domain_target])
            .arg(&plist_path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // launchctl bootstrap returns error 37 if already loaded, which is fine
            if !stderr.contains("already loaded")
                && !stderr.contains("service already loaded")
                && !stderr.contains("37: Operation already in progress")
            {
                return Err(AirlockError::ServiceOperation(format!(
                    "Failed to load service: {}",
                    stderr
                )));
            }
        }

        tracing::info!("Loaded launchd service");
        Ok(())
    }

    /// Load the service (start it via the system service manager).
    #[cfg(target_os = "linux")]
    pub fn load(&self) -> Result<()> {
        let unit_path = self.systemd_unit_path();
        if !unit_path.exists() {
            return Err(AirlockError::ServiceNotInstalled);
        }

        // Reload systemd to pick up any changes
        let output = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("Failed to reload systemd: {}", stderr);
        }

        // Enable and start the service
        let output = std::process::Command::new("systemctl")
            .args(["--user", "enable", "--now", "airlockd.service"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AirlockError::ServiceOperation(format!(
                "Failed to start service: {}",
                stderr
            )));
        }

        tracing::info!("Started systemd service");
        Ok(())
    }

    /// Load the service (unsupported platform).
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub fn load(&self) -> Result<()> {
        Err(AirlockError::Unsupported(
            "Service management is only supported on macOS and Linux".into(),
        ))
    }

    /// Unload the service (stop it via the system service manager).
    #[cfg(target_os = "macos")]
    pub fn unload(&self) -> Result<()> {
        // SAFETY: getuid() is always safe to call on Unix
        let uid = unsafe { libc::getuid() };
        let service_target = format!("gui/{uid}/dev.airlock.daemon");

        let output = std::process::Command::new("launchctl")
            .args(["bootout", &service_target])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Ignore errors when the service isn't loaded
            if !stderr.contains("Could not find specified service")
                && !stderr.contains("No such process")
                && !stderr.contains("3: No such process")
                && !stderr.contains("113:")
            {
                return Err(AirlockError::ServiceOperation(format!(
                    "Failed to unload service: {}",
                    stderr
                )));
            }
        }

        tracing::info!("Unloaded launchd service");
        Ok(())
    }

    /// Unload the service (stop it via the system service manager).
    #[cfg(target_os = "linux")]
    pub fn unload(&self) -> Result<()> {
        // Stop and disable the service
        let output = std::process::Command::new("systemctl")
            .args(["--user", "disable", "--now", "airlockd.service"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Ignore "not loaded" errors
            if !stderr.contains("not loaded") && !stderr.contains("No such file") {
                return Err(AirlockError::ServiceOperation(format!(
                    "Failed to stop service: {}",
                    stderr
                )));
            }
        }

        tracing::info!("Stopped systemd service");
        Ok(())
    }

    /// Unload the service (unsupported platform).
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub fn unload(&self) -> Result<()> {
        Err(AirlockError::Unsupported(
            "Service management is only supported on macOS and Linux".into(),
        ))
    }

    /// Check if the daemon is running via the system service manager.
    #[cfg(target_os = "macos")]
    pub fn is_running(&self) -> Result<bool> {
        let output = std::process::Command::new("launchctl")
            .args(["list", "dev.airlock.daemon"])
            .output()?;

        // If the command succeeds, the service is loaded
        // Check if it's actually running by looking at the PID
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // launchctl list output: "PID\tStatus\tLabel"
            // If PID is "-", the service is loaded but not running
            let first_field = stdout.split_whitespace().next().unwrap_or("-");
            return Ok(first_field != "-" && first_field.parse::<u32>().is_ok());
        }

        Ok(false)
    }

    /// Check if the daemon is running via the system service manager.
    #[cfg(target_os = "linux")]
    pub fn is_running(&self) -> Result<bool> {
        let output = std::process::Command::new("systemctl")
            .args(["--user", "is-active", "airlockd.service"])
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.trim() == "active")
    }

    /// Check if the daemon is running (unsupported platform).
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub fn is_running(&self) -> Result<bool> {
        // Fall back to socket check
        Ok(self.paths.socket().exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_manager_creation() {
        let daemon_path = PathBuf::from("/usr/local/bin/airlockd");
        let manager = ServiceManager::new(daemon_path.clone()).unwrap();

        assert_eq!(manager.daemon_path, daemon_path);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_launchd_plist_path() {
        let daemon_path = PathBuf::from("/usr/local/bin/airlockd");
        let manager = ServiceManager::new(daemon_path).unwrap();

        let plist_path = manager.launchd_plist_path();
        assert!(plist_path
            .to_string_lossy()
            .ends_with("Library/LaunchAgents/dev.airlock.daemon.plist"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_generate_launchd_plist() {
        let daemon_path = PathBuf::from("/usr/local/bin/airlockd");
        let manager = ServiceManager::new(daemon_path).unwrap();

        let plist = manager.generate_launchd_plist();
        assert!(plist.contains("/usr/local/bin/airlockd"));
        assert!(plist.contains("dev.airlock.daemon"));
        assert!(!plist.contains("{{DAEMON_PATH}}"));
        assert!(!plist.contains("{{HOME}}"));
        // No PATH injection — daemon relies on launchd's default PATH
        assert!(!plist.contains("{{PATH}}"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_systemd_unit_path() {
        let daemon_path = PathBuf::from("/usr/local/bin/airlockd");
        let manager = ServiceManager::new(daemon_path).unwrap();

        let unit_path = manager.systemd_unit_path();
        assert!(unit_path
            .to_string_lossy()
            .ends_with(".config/systemd/user/airlockd.service"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_generate_systemd_unit() {
        let daemon_path = PathBuf::from("/usr/local/bin/airlockd");
        let manager = ServiceManager::new(daemon_path).unwrap();

        let unit = manager.generate_systemd_unit();
        assert!(unit.contains("/usr/local/bin/airlockd"));
        assert!(unit.contains("Airlock Daemon"));
        assert!(!unit.contains("{{DAEMON_PATH}}"));
        // No PATH injection — daemon relies on systemd's default PATH
        assert!(!unit.contains("{{PATH}}"));
    }
}
