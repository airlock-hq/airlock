//! Configuration file loading utilities.
//!
//! This module provides functions to load global configuration and
//! workflow configurations from disk or from git tree objects.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{AirlockError, Result};
use crate::git::show_file;
use crate::paths::AirlockPaths;

use super::workflow::{branch_matches_trigger, WorkflowConfig};
use super::GlobalConfig;

/// Load global configuration from a file.
pub fn load_global_config(path: &Path) -> Result<GlobalConfig> {
    let content = std::fs::read_to_string(path)?;
    let config: GlobalConfig = serde_yaml::from_str(&content)
        .map_err(|e| AirlockError::Config(format!("Failed to parse global config: {e}")))?;
    Ok(config)
}

/// Resolve the global config path.
///
/// When no explicit path is provided, this uses [`AirlockPaths::new`], which
/// respects `AIRLOCK_HOME`.
pub fn resolve_global_config_path(config_path: Option<&Path>) -> Option<PathBuf> {
    config_path
        .map(|path| path.to_path_buf())
        .or_else(|| AirlockPaths::new().ok().map(|paths| paths.global_config()))
}

/// Resolve which agent adapter name to use.
///
/// Priority: explicit override → `AIRLOCK_AGENT_ADAPTER` env var → global config → `"auto"`.
pub fn resolve_agent_adapter_name(cli_adapter: Option<&str>, config_path: Option<&Path>) -> String {
    if let Some(name) = cli_adapter {
        return name.to_string();
    }

    if let Ok(name) = std::env::var("AIRLOCK_AGENT_ADAPTER") {
        if !name.is_empty() {
            return name;
        }
    }

    let resolved_config_path = resolve_global_config_path(config_path);

    if let Some(path) = resolved_config_path.as_deref() {
        if path.exists() {
            if let Ok(config) = load_global_config(path) {
                return config.agent.adapter;
            }
        }
    }

    "auto".to_string()
}

/// Parse a single workflow config from a YAML string.
pub fn parse_workflow_config(content: &str) -> Result<WorkflowConfig> {
    let config: WorkflowConfig = serde_yaml::from_str(content)
        .map_err(|e| AirlockError::Config(format!("Failed to parse workflow config: {e}")))?;
    Ok(config)
}

/// Load all workflow configs from `.airlock/workflows/` in the given commit of a git repo.
///
/// Reads workflow files by listing the directory with `git ls-tree` and then
/// reading each `.yml`/`.yaml` file with `git show`.
///
/// Returns a vec of `(filename, WorkflowConfig)` pairs.
pub fn load_workflows_from_tree(
    repo_path: &Path,
    commit: &str,
) -> Result<Vec<(String, WorkflowConfig)>> {
    let workflows_dir = ".airlock/workflows";

    // List files in the workflows directory using git ls-tree
    let output = Command::new("git")
        .args([
            "-C",
            &repo_path.to_string_lossy(),
            "ls-tree",
            "--name-only",
            commit,
            &format!("{}/", workflows_dir),
        ])
        .output()
        .map_err(|e| AirlockError::Git(format!("Failed to run git ls-tree: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AirlockError::Git(format!(
            "git ls-tree for '{}' at commit {} failed (exit {}): {}",
            workflows_dir,
            commit,
            output.status.code().unwrap_or(-1),
            stderr.trim()
        )));
    }

    let listing = String::from_utf8(output.stdout)
        .map_err(|e| AirlockError::Git(format!("git ls-tree output is not valid UTF-8: {e}")))?;

    let mut workflows = Vec::new();

    for line in listing.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Extract just the filename from the full path
        let filename = line.rsplit('/').next().unwrap_or(line);

        if !filename.ends_with(".yml") && !filename.ends_with(".yaml") {
            continue;
        }

        // Read file content from the commit
        let file_path = format!("{}/{}", workflows_dir, filename);
        let content = show_file(repo_path, commit, &file_path).map_err(|e| {
            AirlockError::Config(format!(
                "Failed to read workflow file '{}': {}",
                filename, e
            ))
        })?;

        let config = parse_workflow_config(&content).map_err(|e| {
            AirlockError::Config(format!(
                "Failed to parse workflow file '{}': {}",
                filename, e
            ))
        })?;

        workflows.push((filename.to_string(), config));
    }

    Ok(workflows)
}

/// Load all workflow configs from `.airlock/workflows/` on disk.
///
/// This reads from the filesystem (for CLI use, init, etc.).
pub fn load_workflows_from_disk(repo_root: &Path) -> Result<Vec<(String, WorkflowConfig)>> {
    let workflows_dir = repo_root.join(".airlock").join("workflows");

    if !workflows_dir.exists() {
        return Ok(vec![]);
    }

    let mut workflows = Vec::new();

    let entries = std::fs::read_dir(&workflows_dir).map_err(|e| {
        AirlockError::Config(format!(
            "Failed to read workflows directory '{}': {}",
            workflows_dir.display(),
            e
        ))
    })?;

    for entry in entries {
        let entry = entry
            .map_err(|e| AirlockError::Config(format!("Failed to read directory entry: {e}")))?;

        let path = entry.path();
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        if !filename.ends_with(".yml") && !filename.ends_with(".yaml") {
            continue;
        }

        let content = std::fs::read_to_string(&path).map_err(|e| {
            AirlockError::Config(format!(
                "Failed to read workflow file '{}': {}",
                filename, e
            ))
        })?;

        let config = parse_workflow_config(&content).map_err(|e| {
            AirlockError::Config(format!(
                "Failed to parse workflow file '{}': {}",
                filename, e
            ))
        })?;

        workflows.push((filename, config));
    }

    Ok(workflows)
}

/// Filter workflows to only those matching the given branch.
pub fn filter_workflows_for_branch(
    workflows: Vec<(String, WorkflowConfig)>,
    branch: &str,
) -> Vec<(String, WorkflowConfig)> {
    workflows
        .into_iter()
        .filter(|(_, config)| branch_matches_trigger(branch, &config.on))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn restore_env_var(name: &str, saved: Option<String>) {
        match saved {
            Some(value) => unsafe {
                std::env::set_var(name, value);
            },
            None => unsafe {
                std::env::remove_var(name);
            },
        }
    }

    #[test]
    fn test_resolve_agent_adapter_name_prefers_cli_override() {
        let _guard = env_lock();
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yml");
        std::fs::write(&config_path, "agent:\n  adapter: codex\n").unwrap();
        let saved = std::env::var("AIRLOCK_AGENT_ADAPTER").ok();

        unsafe {
            std::env::set_var("AIRLOCK_AGENT_ADAPTER", "claude-code");
        }

        let adapter = resolve_agent_adapter_name(Some("opencode"), Some(&config_path));

        restore_env_var("AIRLOCK_AGENT_ADAPTER", saved);

        assert_eq!(adapter, "opencode");
    }

    #[test]
    fn test_resolve_agent_adapter_name_prefers_env_over_config() {
        let _guard = env_lock();
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yml");
        std::fs::write(&config_path, "agent:\n  adapter: codex\n").unwrap();
        let saved = std::env::var("AIRLOCK_AGENT_ADAPTER").ok();

        unsafe {
            std::env::set_var("AIRLOCK_AGENT_ADAPTER", "claude-code");
        }

        let adapter = resolve_agent_adapter_name(None, Some(&config_path));

        restore_env_var("AIRLOCK_AGENT_ADAPTER", saved);

        assert_eq!(adapter, "claude-code");
    }

    #[test]
    fn test_resolve_agent_adapter_name_uses_config_when_present() {
        let _guard = env_lock();
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yml");
        std::fs::write(&config_path, "agent:\n  adapter: codex\n").unwrap();
        let saved = std::env::var("AIRLOCK_AGENT_ADAPTER").ok();

        unsafe {
            std::env::remove_var("AIRLOCK_AGENT_ADAPTER");
        }

        let adapter = resolve_agent_adapter_name(None, Some(&config_path));

        restore_env_var("AIRLOCK_AGENT_ADAPTER", saved);

        assert_eq!(adapter, "codex");
    }

    #[test]
    fn test_resolve_agent_adapter_name_falls_back_to_auto() {
        let _guard = env_lock();
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("missing.yml");
        let saved = std::env::var("AIRLOCK_AGENT_ADAPTER").ok();

        unsafe {
            std::env::remove_var("AIRLOCK_AGENT_ADAPTER");
        }

        let adapter = resolve_agent_adapter_name(None, Some(&config_path));

        restore_env_var("AIRLOCK_AGENT_ADAPTER", saved);

        assert_eq!(adapter, "auto");
    }

    #[test]
    fn test_resolve_agent_adapter_name_honors_airlock_home_for_default_config_path() {
        let _guard = env_lock();
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yml");
        std::fs::write(&config_path, "agent:\n  adapter: codex\n").unwrap();
        let saved_home = std::env::var("AIRLOCK_HOME").ok();
        let saved_adapter = std::env::var("AIRLOCK_AGENT_ADAPTER").ok();

        unsafe {
            std::env::set_var("AIRLOCK_HOME", temp_dir.path());
            std::env::remove_var("AIRLOCK_AGENT_ADAPTER");
        }

        let adapter = resolve_agent_adapter_name(None, None);

        restore_env_var("AIRLOCK_HOME", saved_home);
        restore_env_var("AIRLOCK_AGENT_ADAPTER", saved_adapter);

        assert_eq!(adapter, "codex");
    }

    #[test]
    fn test_load_global_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yml");

        let config_content = r#"
sync:
  on_fetch: true
"#;
        std::fs::write(&config_path, config_content).unwrap();

        let config = load_global_config(&config_path).unwrap();
        assert!(config.sync.on_fetch);
    }

    #[test]
    fn test_parse_workflow_config() {
        let yaml = r#"
name: Test
jobs:
  default:
    steps:
      - name: test
        run: cargo test
"#;
        let config = parse_workflow_config(yaml).unwrap();
        assert_eq!(config.name, Some("Test".to_string()));
        assert_eq!(config.jobs.len(), 1);
    }

    #[test]
    fn test_parse_workflow_config_invalid_yaml() {
        let yaml = "{{invalid yaml";
        let result = parse_workflow_config(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_workflows_from_disk() {
        let temp_dir = TempDir::new().unwrap();
        let workflows_dir = temp_dir.path().join(".airlock").join("workflows");
        std::fs::create_dir_all(&workflows_dir).unwrap();

        // Create a main workflow
        std::fs::write(
            workflows_dir.join("main.yml"),
            r#"
name: Main
on:
  push:
    branches: ['**']
jobs:
  default:
    steps:
      - name: test
        run: cargo test
"#,
        )
        .unwrap();

        // Create a hotfix workflow
        std::fs::write(
            workflows_dir.join("hotfix.yaml"),
            r#"
name: Hotfix
on:
  push:
    branches: ['hotfix/**']
jobs:
  default:
    steps:
      - name: push
        run: git push
"#,
        )
        .unwrap();

        // Create a non-yaml file (should be ignored)
        std::fs::write(workflows_dir.join("README.md"), "# Workflows").unwrap();

        let workflows = load_workflows_from_disk(temp_dir.path()).unwrap();
        assert_eq!(workflows.len(), 2);

        let filenames: Vec<&str> = workflows.iter().map(|(f, _)| f.as_str()).collect();
        assert!(filenames.contains(&"main.yml"));
        assert!(filenames.contains(&"hotfix.yaml"));
    }

    #[test]
    fn test_load_workflows_from_disk_no_directory() {
        let temp_dir = TempDir::new().unwrap();
        let workflows = load_workflows_from_disk(temp_dir.path()).unwrap();
        assert!(workflows.is_empty());
    }

    #[test]
    fn test_filter_workflows_for_branch() {
        let workflows = vec![
            (
                "main.yml".to_string(),
                serde_yaml::from_str::<WorkflowConfig>(
                    r#"
name: Main
on:
  push:
    branches: ['**']
jobs:
  default:
    steps:
      - name: test
        run: cargo test
"#,
                )
                .unwrap(),
            ),
            (
                "hotfix.yml".to_string(),
                serde_yaml::from_str::<WorkflowConfig>(
                    r#"
name: Hotfix
on:
  push:
    branches: ['hotfix/**']
jobs:
  default:
    steps:
      - name: push
        run: git push
"#,
                )
                .unwrap(),
            ),
        ];

        // main branch should match only "main.yml"
        let filtered = filter_workflows_for_branch(workflows.clone(), "main");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].0, "main.yml");

        // hotfix/urgent should match both
        let filtered = filter_workflows_for_branch(workflows.clone(), "hotfix/urgent");
        assert_eq!(filtered.len(), 2);

        // feature/foo should match only "main.yml"
        let filtered = filter_workflows_for_branch(workflows, "feature/foo");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].0, "main.yml");
    }

    #[test]
    fn test_filter_workflows_no_trigger_matches_all() {
        let workflows = vec![(
            "notrigger.yml".to_string(),
            serde_yaml::from_str::<WorkflowConfig>(
                r#"
jobs:
  default:
    steps:
      - name: test
        run: cargo test
"#,
            )
            .unwrap(),
        )];

        let filtered = filter_workflows_for_branch(workflows, "any-branch");
        assert_eq!(filtered.len(), 1);
    }
}
