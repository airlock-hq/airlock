//! AirlockPaths tests for socket.

use airlock_core::AirlockPaths;
use tempfile::TempDir;

#[test]
fn test_socket_path() {
    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());

    #[cfg(unix)]
    {
        let socket_path = paths.socket();
        assert!(socket_path.to_string_lossy().ends_with("socket"));
    }

    let socket_name = paths.socket_name();
    #[cfg(unix)]
    assert!(socket_name.ends_with("socket"));
    #[cfg(windows)]
    assert_eq!(socket_name, "airlock-daemon");
}

#[test]
fn test_paths_ensure_dirs() {
    let temp_dir = TempDir::new().unwrap();
    let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());

    // Directories should not exist initially
    assert!(!paths.repos_dir().exists());
    assert!(!paths.artifacts_dir().exists());
    assert!(!paths.locks_dir().exists());

    // Create directories
    paths.ensure_dirs().unwrap();

    // Directories should exist now
    assert!(paths.repos_dir().exists());
    assert!(paths.artifacts_dir().exists());
    assert!(paths.locks_dir().exists());
}
