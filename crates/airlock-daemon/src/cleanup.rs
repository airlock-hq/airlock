//! Artifact cleanup utilities.
//!
//! This module provides functions to clean up old artifacts based on the
//! `max_artifact_age_days` setting in the global configuration.

use airlock_core::{load_global_config, AirlockPaths, GlobalConfig};
use std::path::Path;
use std::time::{Duration, SystemTime};
use tracing::{debug, info, warn};

/// Result of the artifact cleanup operation.
#[derive(Debug, Default)]
pub struct CleanupResult {
    /// Number of artifact directories deleted.
    pub deleted_count: u32,
    /// Number of artifact directories preserved.
    pub preserved_count: u32,
    /// Total bytes freed (approximate).
    pub bytes_freed: u64,
    /// Errors encountered during cleanup (non-fatal).
    pub errors: Vec<String>,
}

/// Clean up old artifacts based on the `max_artifact_age_days` configuration.
///
/// This function:
/// 1. Reads the `max_artifact_age_days` from the global config
/// 2. Enumerates all artifact directories under `~/.airlock/artifacts/`
/// 3. Deletes directories older than the configured threshold
///
/// # Arguments
/// * `paths` - The AirlockPaths instance providing directory locations
///
/// # Returns
/// A `CleanupResult` with statistics about the cleanup operation.
pub fn cleanup_old_artifacts(paths: &AirlockPaths) -> CleanupResult {
    let mut result = CleanupResult::default();

    // Load global config to get max_artifact_age_days
    let max_age_days = load_max_artifact_age_days(paths);

    if max_age_days == 0 {
        info!("Artifact cleanup disabled (max_artifact_age_days = 0)");
        return result;
    }

    info!(
        "Running artifact cleanup (max_artifact_age_days = {})",
        max_age_days
    );

    let artifacts_dir = paths.artifacts_dir();
    if !artifacts_dir.exists() {
        debug!("Artifacts directory does not exist, nothing to clean up");
        return result;
    }

    let max_age = Duration::from_secs(max_age_days as u64 * 24 * 60 * 60);
    let now = SystemTime::now();

    // Enumerate repo directories
    let repo_dirs = match std::fs::read_dir(&artifacts_dir) {
        Ok(dirs) => dirs,
        Err(e) => {
            result
                .errors
                .push(format!("Failed to read artifacts directory: {}", e));
            return result;
        }
    };

    for repo_entry in repo_dirs.flatten() {
        let repo_path = repo_entry.path();
        if !repo_path.is_dir() {
            continue;
        }

        // Enumerate run directories within each repo
        let run_dirs = match std::fs::read_dir(&repo_path) {
            Ok(dirs) => dirs,
            Err(e) => {
                result.errors.push(format!(
                    "Failed to read repo artifacts directory {:?}: {}",
                    repo_path, e
                ));
                continue;
            }
        };

        for run_entry in run_dirs.flatten() {
            let run_path = run_entry.path();
            if !run_path.is_dir() {
                continue;
            }

            // Check if the directory is older than max_age
            match is_directory_older_than(&run_path, now, max_age) {
                Ok(true) => {
                    // Directory is old, delete it
                    let size = calculate_directory_size(&run_path);
                    match std::fs::remove_dir_all(&run_path) {
                        Ok(()) => {
                            debug!("Deleted old artifact directory: {:?}", run_path);
                            result.deleted_count += 1;
                            result.bytes_freed += size;
                        }
                        Err(e) => {
                            result.errors.push(format!(
                                "Failed to delete artifact directory {:?}: {}",
                                run_path, e
                            ));
                        }
                    }
                }
                Ok(false) => {
                    // Directory is recent, preserve it
                    result.preserved_count += 1;
                }
                Err(e) => {
                    result
                        .errors
                        .push(format!("Failed to check age of {:?}: {}", run_path, e));
                    result.preserved_count += 1; // Preserve on error
                }
            }
        }

        // Clean up empty repo directories
        if is_directory_empty(&repo_path) {
            if let Err(e) = std::fs::remove_dir(&repo_path) {
                debug!(
                    "Failed to remove empty repo directory {:?}: {}",
                    repo_path, e
                );
            } else {
                debug!("Removed empty repo directory: {:?}", repo_path);
            }
        }
    }

    if result.deleted_count > 0 {
        info!(
            "Artifact cleanup complete: deleted {} directories, freed {} bytes, preserved {}",
            result.deleted_count, result.bytes_freed, result.preserved_count
        );
    } else {
        info!(
            "Artifact cleanup complete: no old artifacts to delete, preserved {}",
            result.preserved_count
        );
    }

    if !result.errors.is_empty() {
        warn!(
            "Artifact cleanup encountered {} errors",
            result.errors.len()
        );
    }

    result
}

/// Load the max_artifact_age_days from global config.
/// Returns the default (30 days) if config cannot be loaded.
fn load_max_artifact_age_days(paths: &AirlockPaths) -> u32 {
    let global_config_path = paths.global_config();

    if !global_config_path.exists() {
        debug!("Global config not found, using default max_artifact_age_days");
        return GlobalConfig::default().storage.max_artifact_age_days;
    }

    match load_global_config(&global_config_path) {
        Ok(config) => {
            debug!(
                "Loaded max_artifact_age_days = {} from config",
                config.storage.max_artifact_age_days
            );
            config.storage.max_artifact_age_days
        }
        Err(e) => {
            warn!(
                "Failed to load global config, using default max_artifact_age_days: {}",
                e
            );
            GlobalConfig::default().storage.max_artifact_age_days
        }
    }
}

/// Check if a directory's modification time is older than the given duration.
fn is_directory_older_than(
    path: &Path,
    now: SystemTime,
    max_age: Duration,
) -> std::io::Result<bool> {
    let metadata = std::fs::metadata(path)?;
    let modified = metadata.modified()?;

    // Calculate age
    let age = now.duration_since(modified).unwrap_or(Duration::ZERO);

    Ok(age > max_age)
}

/// Calculate the total size of a directory (recursive).
fn calculate_directory_size(path: &Path) -> u64 {
    let mut total = 0;

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_file() {
                if let Ok(metadata) = entry_path.metadata() {
                    total += metadata.len();
                }
            } else if entry_path.is_dir() {
                total += calculate_directory_size(&entry_path);
            }
        }
    }

    total
}

/// Check if a directory is empty.
fn is_directory_empty(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cleanup_old_artifacts_removes_old_directories() {
        let temp_dir = TempDir::new().unwrap();
        let airlock_home = temp_dir.path().to_path_buf();
        let paths = AirlockPaths::with_root(airlock_home.clone());
        paths.ensure_dirs().unwrap();

        // Create global config with max_artifact_age_days = 7
        let config_content = r#"
storage:
  max_artifact_age_days: 7
"#;
        std::fs::write(paths.global_config(), config_content).unwrap();

        // Create test repo and run directories
        let repo_id = "test-repo";
        let old_run_id = "old-run";
        let new_run_id = "new-run";

        let old_artifact_path = paths.run_artifacts(repo_id, old_run_id);
        let new_artifact_path = paths.run_artifacts(repo_id, new_run_id);

        std::fs::create_dir_all(&old_artifact_path).unwrap();
        std::fs::create_dir_all(&new_artifact_path).unwrap();

        // Create a file in each directory
        std::fs::write(old_artifact_path.join("test.json"), "{}").unwrap();
        std::fs::write(new_artifact_path.join("test.json"), "{}").unwrap();

        // Set old directory mtime to 10 days ago
        let now = std::time::SystemTime::now();
        let ten_days_ago = now - std::time::Duration::from_secs(10 * 24 * 60 * 60);
        filetime::set_file_mtime(
            &old_artifact_path,
            filetime::FileTime::from_system_time(ten_days_ago),
        )
        .unwrap();

        // Run cleanup
        let result = cleanup_old_artifacts(&paths);

        // Verify results
        assert_eq!(result.deleted_count, 1, "Should delete 1 old directory");
        assert_eq!(result.preserved_count, 1, "Should preserve 1 new directory");
        assert!(
            !old_artifact_path.exists(),
            "Old artifact should be deleted"
        );
        assert!(
            new_artifact_path.exists(),
            "New artifact should be preserved"
        );
    }

    #[test]
    fn test_cleanup_preserves_recent_artifacts() {
        let temp_dir = TempDir::new().unwrap();
        let airlock_home = temp_dir.path().to_path_buf();
        let paths = AirlockPaths::with_root(airlock_home.clone());
        paths.ensure_dirs().unwrap();

        // Create global config with max_artifact_age_days = 7
        let config_content = r#"
storage:
  max_artifact_age_days: 7
"#;
        std::fs::write(paths.global_config(), config_content).unwrap();

        // Create test repo and run directories
        let repo_id = "test-repo";
        let recent_run_id = "recent-run";

        let recent_artifact_path = paths.run_artifacts(repo_id, recent_run_id);
        std::fs::create_dir_all(&recent_artifact_path).unwrap();
        std::fs::write(recent_artifact_path.join("test.json"), "{}").unwrap();

        // Set mtime to 3 days ago (within threshold)
        let now = std::time::SystemTime::now();
        let three_days_ago = now - std::time::Duration::from_secs(3 * 24 * 60 * 60);
        filetime::set_file_mtime(
            &recent_artifact_path,
            filetime::FileTime::from_system_time(three_days_ago),
        )
        .unwrap();

        // Run cleanup
        let result = cleanup_old_artifacts(&paths);

        // Verify results
        assert_eq!(result.deleted_count, 0, "Should not delete any directories");
        assert_eq!(result.preserved_count, 1, "Should preserve 1 directory");
        assert!(
            recent_artifact_path.exists(),
            "Recent artifact should be preserved"
        );
    }

    #[test]
    fn test_cleanup_disabled_with_zero_age() {
        let temp_dir = TempDir::new().unwrap();
        let airlock_home = temp_dir.path().to_path_buf();
        let paths = AirlockPaths::with_root(airlock_home.clone());
        paths.ensure_dirs().unwrap();

        // Create global config with max_artifact_age_days = 0 (disabled)
        let config_content = r#"
storage:
  max_artifact_age_days: 0
"#;
        std::fs::write(paths.global_config(), config_content).unwrap();

        // Create test artifact directory
        let repo_id = "test-repo";
        let run_id = "test-run";
        let artifact_path = paths.run_artifacts(repo_id, run_id);
        std::fs::create_dir_all(&artifact_path).unwrap();

        // Set mtime to very old (100 days ago)
        let now = std::time::SystemTime::now();
        let old_time = now - std::time::Duration::from_secs(100 * 24 * 60 * 60);
        filetime::set_file_mtime(
            &artifact_path,
            filetime::FileTime::from_system_time(old_time),
        )
        .unwrap();

        // Run cleanup
        let result = cleanup_old_artifacts(&paths);

        // Verify results - nothing should be deleted
        assert_eq!(
            result.deleted_count, 0,
            "Should not delete any directories when disabled"
        );
        assert_eq!(
            result.preserved_count, 0,
            "Should not process directories when disabled"
        );
        assert!(artifact_path.exists(), "Artifact should be preserved");
    }

    #[test]
    fn test_cleanup_uses_default_when_no_config() {
        let temp_dir = TempDir::new().unwrap();
        let airlock_home = temp_dir.path().to_path_buf();
        let paths = AirlockPaths::with_root(airlock_home.clone());
        paths.ensure_dirs().unwrap();

        // No config file created - should use default of 30 days

        // Create test artifact directory
        let repo_id = "test-repo";
        let run_id = "test-run";
        let artifact_path = paths.run_artifacts(repo_id, run_id);
        std::fs::create_dir_all(&artifact_path).unwrap();

        // Set mtime to 40 days ago (older than default 30 days)
        let now = std::time::SystemTime::now();
        let old_time = now - std::time::Duration::from_secs(40 * 24 * 60 * 60);
        filetime::set_file_mtime(
            &artifact_path,
            filetime::FileTime::from_system_time(old_time),
        )
        .unwrap();

        // Run cleanup
        let result = cleanup_old_artifacts(&paths);

        // Verify results - should be deleted (default 30 days threshold)
        assert_eq!(
            result.deleted_count, 1,
            "Should delete 1 directory with default threshold"
        );
        assert!(!artifact_path.exists(), "Old artifact should be deleted");
    }

    #[test]
    fn test_cleanup_removes_empty_repo_directories() {
        let temp_dir = TempDir::new().unwrap();
        let airlock_home = temp_dir.path().to_path_buf();
        let paths = AirlockPaths::with_root(airlock_home.clone());
        paths.ensure_dirs().unwrap();

        // Create global config with max_artifact_age_days = 1
        let config_content = r#"
storage:
  max_artifact_age_days: 1
"#;
        std::fs::write(paths.global_config(), config_content).unwrap();

        // Create test repo with a single old run
        let repo_id = "empty-repo";
        let run_id = "old-run";
        let artifact_path = paths.run_artifacts(repo_id, run_id);
        let repo_path = paths.repo_artifacts(repo_id);
        std::fs::create_dir_all(&artifact_path).unwrap();

        // Set mtime to old (5 days ago)
        let now = std::time::SystemTime::now();
        let old_time = now - std::time::Duration::from_secs(5 * 24 * 60 * 60);
        filetime::set_file_mtime(
            &artifact_path,
            filetime::FileTime::from_system_time(old_time),
        )
        .unwrap();

        // Run cleanup
        let result = cleanup_old_artifacts(&paths);

        // Verify results
        assert_eq!(result.deleted_count, 1, "Should delete 1 run directory");
        assert!(!artifact_path.exists(), "Run directory should be deleted");
        assert!(
            !repo_path.exists(),
            "Empty repo directory should also be removed"
        );
    }

    #[test]
    fn test_cleanup_handles_missing_artifacts_dir() {
        let temp_dir = TempDir::new().unwrap();
        let airlock_home = temp_dir.path().to_path_buf();
        let paths = AirlockPaths::with_root(airlock_home.clone());
        // Don't call ensure_dirs() - artifacts dir won't exist

        let result = cleanup_old_artifacts(&paths);

        // Should not error, just return empty result
        assert_eq!(result.deleted_count, 0);
        assert_eq!(result.preserved_count, 0);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_is_directory_older_than() {
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path().join("test");
        std::fs::create_dir(&test_dir).unwrap();

        let now = std::time::SystemTime::now();
        let max_age = Duration::from_secs(7 * 24 * 60 * 60); // 7 days

        // Set mtime to 10 days ago
        let ten_days_ago = now - std::time::Duration::from_secs(10 * 24 * 60 * 60);
        filetime::set_file_mtime(
            &test_dir,
            filetime::FileTime::from_system_time(ten_days_ago),
        )
        .unwrap();

        assert!(is_directory_older_than(&test_dir, now, max_age).unwrap());

        // Set mtime to 3 days ago
        let three_days_ago = now - std::time::Duration::from_secs(3 * 24 * 60 * 60);
        filetime::set_file_mtime(
            &test_dir,
            filetime::FileTime::from_system_time(three_days_ago),
        )
        .unwrap();

        assert!(!is_directory_older_than(&test_dir, now, max_age).unwrap());
    }

    #[test]
    fn test_calculate_directory_size() {
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path().join("test");
        std::fs::create_dir(&test_dir).unwrap();

        // Create files with known sizes
        std::fs::write(test_dir.join("file1.txt"), "hello").unwrap(); // 5 bytes
        std::fs::write(test_dir.join("file2.txt"), "world!").unwrap(); // 6 bytes

        let subdir = test_dir.join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(subdir.join("nested.txt"), "nested content").unwrap(); // 14 bytes

        let size = calculate_directory_size(&test_dir);
        assert_eq!(size, 25); // 5 + 6 + 14 = 25 bytes
    }

    #[test]
    fn test_is_directory_empty() {
        let temp_dir = TempDir::new().unwrap();

        let empty_dir = temp_dir.path().join("empty");
        std::fs::create_dir(&empty_dir).unwrap();
        assert!(is_directory_empty(&empty_dir));

        let non_empty_dir = temp_dir.path().join("non_empty");
        std::fs::create_dir(&non_empty_dir).unwrap();
        std::fs::write(non_empty_dir.join("file.txt"), "content").unwrap();
        assert!(!is_directory_empty(&non_empty_dir));
    }
}
