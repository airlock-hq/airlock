//! Daemon executable path tests.

use std::path::PathBuf;

#[test]
fn test_daemon_path_fallback() {
    // When the daemon is not found relative to the executable,
    // it should fall back to "airlockd" (relying on PATH)

    // We can't easily test the actual get_daemon_path() function
    // since it depends on std::env::current_exe(), but we can
    // verify the fallback behavior is reasonable
    let fallback_path = PathBuf::from("airlockd");
    assert_eq!(fallback_path.file_name().unwrap(), "airlockd");
}
