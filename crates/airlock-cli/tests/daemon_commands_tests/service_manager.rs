//! ServiceManager tests.

use airlock_core::ServiceManager;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_service_manager_creation() {
    let temp_dir = TempDir::new().unwrap();
    let daemon_path = PathBuf::from("/usr/local/bin/airlockd");
    let manager =
        ServiceManager::with_home_dir(daemon_path.clone(), temp_dir.path().to_path_buf()).unwrap();

    // ServiceManager should be created successfully
    assert!(!manager.is_installed());
}

#[test]
#[cfg(target_os = "macos")]
fn test_launchd_plist_path() {
    let temp_dir = TempDir::new().unwrap();
    let daemon_path = PathBuf::from("/usr/local/bin/airlockd");
    let manager =
        ServiceManager::with_home_dir(daemon_path, temp_dir.path().to_path_buf()).unwrap();

    let plist_path = manager.launchd_plist_path();
    assert!(plist_path
        .to_string_lossy()
        .ends_with("Library/LaunchAgents/dev.airlock.daemon.plist"));
}

#[test]
#[cfg(target_os = "linux")]
fn test_systemd_unit_path() {
    let temp_dir = TempDir::new().unwrap();
    let daemon_path = PathBuf::from("/usr/local/bin/airlockd");
    let manager =
        ServiceManager::with_home_dir(daemon_path, temp_dir.path().to_path_buf()).unwrap();

    let unit_path = manager.systemd_unit_path();
    assert!(unit_path
        .to_string_lossy()
        .ends_with(".config/systemd/user/airlockd.service"));
}

#[test]
fn test_service_not_installed() {
    let temp_dir = TempDir::new().unwrap();
    let daemon_path = PathBuf::from("/nonexistent/airlockd");
    let manager =
        ServiceManager::with_home_dir(daemon_path, temp_dir.path().to_path_buf()).unwrap();

    assert!(!manager.is_installed());
}

#[test]
#[cfg(target_os = "macos")]
fn test_install_launchd_plist() {
    let temp_dir = TempDir::new().unwrap();

    let daemon_path = temp_dir.path().join("airlockd");
    std::fs::write(&daemon_path, "#!/bin/bash\necho test").unwrap();

    let manager =
        ServiceManager::with_home_dir(daemon_path.clone(), temp_dir.path().to_path_buf()).unwrap();

    // Service should not be installed initially
    assert!(!manager.is_installed());
}
