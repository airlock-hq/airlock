//! Action loader for reusable actions.
//!
//! This module handles loading and resolving reusable actions referenced via the `uses:` syntax
//! in workflow configurations. Reusable actions are fetched from GitHub and cached locally.
//!
//! ## Action Reference Format
//!
//! The `uses:` field supports the following formats:
//! - `owner/repo/path@version` - Full path within a repository
//!
//! ## Version Specifiers
//!
//! - `@v1` - Latest v1.x.x tag (semver major version)
//! - `@v1.2.3` - Exact semver tag
//! - `@main` or `@master` - Branch HEAD
//! - `@abc123` - Commit SHA (7+ characters)
//!
//! ## Cache Location
//!
//! Cached actions are stored in `~/.airlock/actions/<owner>/<repo>/<path>@<version>/`.
//!
//! ## Action Definition File
//!
//! Each reusable step must contain a `step.yml` file (or `action.yml`/`stage.yaml` as legacy
//! fallback) with the step definition.

use airlock_core::{AirlockPaths, ApprovalMode, StepDefinition};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tracing::{debug, info, warn};

// Bundled first-party defaults — always up-to-date with the binary.
const BUNDLED_REBASE: &str = include_str!("../../../defaults/rebase/step.yml");
const BUNDLED_LINT: &str = include_str!("../../../defaults/lint/step.yml");
const BUNDLED_DOCUMENT: &str = include_str!("../../../defaults/document/step.yml");
const BUNDLED_TEST: &str = include_str!("../../../defaults/test/step.yml");
const BUNDLED_DESCRIBE: &str = include_str!("../../../defaults/describe/step.yml");
const BUNDLED_PUSH: &str = include_str!("../../../defaults/push/step.yml");
const BUNDLED_CRITIQUE: &str = include_str!("../../../defaults/critique/step.yml");
const BUNDLED_GATE: &str = include_str!("../../../defaults/gate/step.yml");
const BUNDLED_CREATE_PR: &str = include_str!("../../../defaults/create-pr/step.yml");

/// TTL for mutable ref caches (branches, semver-major). 1 hour.
const CACHE_TTL_SECS: u64 = 3600;

/// Parsed action reference from `uses:` field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageReference {
    /// Repository owner (GitHub username or org).
    pub owner: String,
    /// Repository name.
    pub repo: String,
    /// Path within the repository (e.g., "defaults/lint").
    pub path: String,
    /// Version specifier.
    pub version: VersionSpec,
}

/// Version specifier for stage references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionSpec {
    /// Major semver version (e.g., "v1" matches v1.x.x).
    SemverMajor(u32),
    /// Exact semver version (e.g., "v1.2.3").
    SemverExact(String),
    /// Branch name (e.g., "main", "master").
    Branch(String),
    /// Commit SHA (at least 7 characters).
    Commit(String),
}

impl VersionSpec {
    /// Returns `true` for refs that can change over time (branches, semver-major).
    /// Immutable refs (exact semver, commit SHA) return `false`.
    pub fn is_mutable(&self) -> bool {
        matches!(self, VersionSpec::Branch(_) | VersionSpec::SemverMajor(_))
    }
}

impl std::fmt::Display for VersionSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VersionSpec::SemverMajor(v) => write!(f, "v{}", v),
            VersionSpec::SemverExact(v) => write!(f, "{}", v),
            VersionSpec::Branch(b) => write!(f, "{}", b),
            VersionSpec::Commit(c) => write!(f, "{}", c),
        }
    }
}

/// Step definition file content (step.yml, or legacy action.yml/stage.yaml).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageYaml {
    /// The shell command to run.
    pub run: String,
    /// Shell to use. When omitted, the executor uses the user's login shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Default environment variables for this reusable step.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Continue pipeline if this step fails.
    #[serde(default, alias = "continue_on_error", rename = "continue-on-error")]
    pub continue_on_error: bool,
    /// Pause for user approval after this step completes.
    #[serde(default, alias = "require_approval", rename = "require-approval")]
    pub require_approval: ApprovalMode,
    /// Maximum execution time in seconds.
    /// When omitted, the executor applies a default timeout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    /// Auto-apply pending patches after this step passes.
    #[serde(default, alias = "apply_patch", rename = "apply-patch")]
    pub apply_patch: bool,
    /// Description of what this action does.
    #[serde(default)]
    pub description: Option<String>,
}

/// Return the bundled YAML for first-party `airlock-hq/airlock/defaults/*@main` refs.
fn get_bundled_default(reference: &StageReference) -> Option<&'static str> {
    if reference.owner != "airlock-hq"
        || reference.repo != "airlock"
        || reference.version != VersionSpec::Branch("main".to_string())
    {
        return None;
    }
    match reference.path.as_str() {
        "defaults/rebase" => Some(BUNDLED_REBASE),
        "defaults/lint" => Some(BUNDLED_LINT),
        "defaults/document" => Some(BUNDLED_DOCUMENT),
        "defaults/test" => Some(BUNDLED_TEST),
        "defaults/describe" => Some(BUNDLED_DESCRIBE),
        "defaults/push" => Some(BUNDLED_PUSH),
        "defaults/critique" => Some(BUNDLED_CRITIQUE),
        "defaults/gate" => Some(BUNDLED_GATE),
        "defaults/create-pr" => Some(BUNDLED_CREATE_PR),
        _ => None,
    }
}

/// Check if a cached directory is older than `ttl_secs`.
/// Returns `true` (stale) when the path doesn't exist or its mtime exceeds the TTL.
fn is_cache_stale(path: &Path, ttl_secs: u64) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return true;
    };
    let Ok(modified) = metadata.modified() else {
        return true;
    };
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    age > Duration::from_secs(ttl_secs)
}

/// Action loader for resolving reusable actions.
pub struct StageLoader {
    /// Cache directory for downloaded actions.
    cache_dir: PathBuf,
}

impl StageLoader {
    /// Create a new action loader with the default cache directory.
    pub fn new() -> Result<Self> {
        let paths =
            AirlockPaths::new().map_err(|e| anyhow!("Failed to get airlock paths: {}", e))?;
        let cache_dir = paths.root().join("actions");
        Ok(Self { cache_dir })
    }

    /// Create an action loader with a custom cache directory.
    #[cfg(test)]
    pub fn with_cache_dir(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    /// Resolve a step definition that uses a reusable action reference.
    ///
    /// This will:
    /// 1. Parse the `uses:` reference
    /// 2. For first-party `airlock-hq/airlock/defaults/*@main`, use the bundled YAML
    /// 3. Otherwise check the local cache (with TTL for mutable refs)
    /// 4. If not cached, fetch from GitHub
    /// 5. Read the step.yml (or legacy action.yml/stage.yaml) file
    /// 6. Merge inline properties (inline properties override step.yml)
    ///
    /// Returns the resolved step definition with the `run` command populated.
    pub async fn resolve_stage(&self, stage: &StepDefinition) -> Result<StepDefinition> {
        let use_ref = stage
            .uses
            .as_ref()
            .ok_or_else(|| anyhow!("Step '{}' has no 'uses' reference to resolve", stage.name))?;

        info!("Resolving reusable action: {}", use_ref);

        // Parse the reference
        let reference = parse_stage_reference(use_ref)
            .with_context(|| format!("Failed to parse action reference: {}", use_ref))?;

        // Fast path: use bundled YAML for first-party defaults (no cache, no network)
        if let Some(bundled_yaml) = get_bundled_default(&reference) {
            debug!("Using bundled default for {}", use_ref);
            let stage_yaml: StageYaml = serde_yaml::from_str(bundled_yaml)
                .with_context(|| format!("Failed to parse bundled YAML for {}", use_ref))?;
            return self.merge_stage(stage, &stage_yaml, None);
        }

        // Get cached path or fetch from GitHub
        let stage_dir = self.get_or_fetch_stage(&reference).await?;

        // Read step.yml (preferred), action.yml (legacy), or stage.yaml (legacy fallback)
        let step_yml_path = stage_dir.join("step.yml");
        let action_yml_path = stage_dir.join("action.yml");
        let stage_yaml_path = stage_dir.join("stage.yaml");
        let yaml_path = if step_yml_path.exists() {
            step_yml_path
        } else if action_yml_path.exists() {
            action_yml_path
        } else {
            stage_yaml_path
        };
        let stage_yaml = self.read_stage_yaml(&yaml_path)?;

        self.merge_stage(stage, &stage_yaml, Some(&stage_dir))
    }

    /// Merge inline step properties with the resolved stage YAML.
    ///
    /// When `stage_dir` is `Some`, relative paths in `run` are resolved against it.
    /// When `stage_dir` is `None` (bundled defaults), `run` is used as-is.
    fn merge_stage(
        &self,
        stage: &StepDefinition,
        stage_yaml: &StageYaml,
        stage_dir: Option<&Path>,
    ) -> Result<StepDefinition> {
        let base_run = if let Some(run) = stage.run.clone() {
            run
        } else if let Some(dir) = stage_dir {
            resolve_run_path(&stage_yaml.run, dir)
        } else {
            // Bundled default — use run command as-is (inline scripts)
            stage_yaml.run.clone()
        };

        let resolved_run = Some(base_run);

        let mut merged_env = stage_yaml.env.clone();
        for (key, value) in &stage.env {
            merged_env.insert(key.clone(), value.clone());
        }

        let resolved = StepDefinition {
            name: stage.name.clone(),
            run: resolved_run,
            uses: stage.uses.clone(),
            // Inline shell overrides if explicitly set, otherwise use step.yml's shell
            shell: stage.shell.clone().or_else(|| stage_yaml.shell.clone()),
            env: merged_env,
            continue_on_error: stage.continue_on_error || stage_yaml.continue_on_error,
            require_approval: if stage.require_approval != ApprovalMode::Never {
                stage.require_approval
            } else {
                stage_yaml.require_approval
            },
            timeout: stage.timeout.or(stage_yaml.timeout),
            apply_patch: stage.apply_patch || stage_yaml.apply_patch,
        };

        debug!("Resolved action '{}': {:?}", stage.name, resolved);
        Ok(resolved)
    }

    /// Get the cached action directory or fetch from GitHub.
    ///
    /// For mutable refs (branches, semver-major), the cache is invalidated after
    /// [`CACHE_TTL_SECS`]. Immutable refs (exact semver, commit SHA) are cached forever.
    async fn get_or_fetch_stage(&self, reference: &StageReference) -> Result<PathBuf> {
        let cache_path = self.cache_path(reference);

        // Check if already cached (step.yml preferred, action.yml/stage.yaml as fallback)
        if cache_path.exists() {
            let step_yml = cache_path.join("step.yml");
            let action_yml = cache_path.join("action.yml");
            let stage_yaml = cache_path.join("stage.yaml");
            if step_yml.exists() || action_yml.exists() || stage_yaml.exists() {
                // For mutable refs, check if the cache has expired
                if reference.version.is_mutable() && is_cache_stale(&cache_path, CACHE_TTL_SECS) {
                    info!("Cache expired for mutable ref, re-fetching");
                    if let Err(e) = std::fs::remove_dir_all(&cache_path) {
                        warn!("Failed to remove stale cache: {}", e);
                        // Graceful fallback to stale cache
                        return Ok(cache_path);
                    }
                    // Falls through to fetch below
                } else {
                    debug!("Using cached action: {}", cache_path.display());
                    return Ok(cache_path);
                }
            }
        }

        // Fetch from GitHub
        info!(
            "Fetching action {}/{}/{}@{} from GitHub",
            reference.owner, reference.repo, reference.path, reference.version
        );

        self.fetch_stage_from_github(reference, &cache_path).await?;

        Ok(cache_path)
    }

    /// Calculate the cache path for an action reference.
    fn cache_path(&self, reference: &StageReference) -> PathBuf {
        self.cache_dir
            .join(&reference.owner)
            .join(&reference.repo)
            .join(format!(
                "{}@{}",
                reference.path.replace('/', "_"),
                reference.version
            ))
    }

    /// Fetch an action from GitHub.
    ///
    /// Uses sparse checkout to clone only the needed action directory, enabling actions
    /// with multiple files (run scripts, helpers, templates, etc.).
    async fn fetch_stage_from_github(
        &self,
        reference: &StageReference,
        cache_path: &PathBuf,
    ) -> Result<()> {
        // Resolve version to a git ref
        let git_ref = self.resolve_version_to_ref(reference).await?;

        // First, try to fetch using sparse checkout (for actions with multiple files)
        if let Ok(()) = self
            .fetch_stage_with_sparse_checkout(reference, cache_path, &git_ref)
            .await
        {
            return Ok(());
        }

        // Fallback: fetch just step.yml using raw content URL
        warn!(
            "Sparse checkout failed, falling back to raw content fetch for {}",
            reference.path
        );
        self.fetch_stage_yaml_only(reference, cache_path, &git_ref)
            .await
    }

    /// Fetch an action using git sparse checkout to get the entire directory.
    async fn fetch_stage_with_sparse_checkout(
        &self,
        reference: &StageReference,
        cache_path: &std::path::Path,
        git_ref: &str,
    ) -> Result<()> {
        use std::process::Command;

        let repo_url = format!(
            "https://github.com/{}/{}.git",
            reference.owner, reference.repo
        );

        // Create a temporary directory for the clone
        let temp_dir =
            tempfile::tempdir().context("Failed to create temporary directory for git clone")?;

        debug!(
            "Cloning {} with sparse checkout to fetch {}",
            repo_url, reference.path
        );

        // Initialize git repo with sparse checkout
        // Set GIT_TERMINAL_PROMPT=0 to prevent interactive credential prompts in daemon
        let init_output = Command::new("git")
            .args(["init"])
            .current_dir(temp_dir.path())
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .context("Failed to run git init")?;

        if !init_output.status.success() {
            let stderr = String::from_utf8_lossy(&init_output.stderr);
            return Err(anyhow!("git init failed: {}", stderr));
        }

        // Add remote
        let remote_output = Command::new("git")
            .args(["remote", "add", "origin", &repo_url])
            .current_dir(temp_dir.path())
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .context("Failed to add git remote")?;

        if !remote_output.status.success() {
            let stderr = String::from_utf8_lossy(&remote_output.stderr);
            return Err(anyhow!("git remote add failed: {}", stderr));
        }

        // Enable sparse checkout
        let sparse_output = Command::new("git")
            .args(["sparse-checkout", "init", "--cone"])
            .current_dir(temp_dir.path())
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .context("Failed to enable sparse checkout")?;

        if !sparse_output.status.success() {
            let stderr = String::from_utf8_lossy(&sparse_output.stderr);
            return Err(anyhow!("git sparse-checkout init failed: {}", stderr));
        }

        // Set sparse checkout path
        let sparse_set_output = Command::new("git")
            .args(["sparse-checkout", "set", &reference.path])
            .current_dir(temp_dir.path())
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .context("Failed to set sparse checkout path")?;

        if !sparse_set_output.status.success() {
            let stderr = String::from_utf8_lossy(&sparse_set_output.stderr);
            return Err(anyhow!("git sparse-checkout set failed: {}", stderr));
        }

        // Fetch only the needed ref with depth=1
        let fetch_output = Command::new("git")
            .args(["fetch", "--depth=1", "origin", git_ref])
            .current_dir(temp_dir.path())
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .context("Failed to fetch from git")?;

        if !fetch_output.status.success() {
            let stderr = String::from_utf8_lossy(&fetch_output.stderr);
            return Err(anyhow!("git fetch failed: {}", stderr));
        }

        // Checkout the ref
        let checkout_output = Command::new("git")
            .args(["checkout", "FETCH_HEAD"])
            .current_dir(temp_dir.path())
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .context("Failed to checkout")?;

        if !checkout_output.status.success() {
            let stderr = String::from_utf8_lossy(&checkout_output.stderr);
            return Err(anyhow!("git checkout failed: {}", stderr));
        }

        // Copy the action directory to cache
        let stage_src = temp_dir.path().join(&reference.path);
        if !stage_src.exists() {
            return Err(anyhow!(
                "Action directory {} not found after checkout",
                reference.path
            ));
        }

        // Ensure parent directories exist
        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create cache parent: {}", parent.display()))?;
        }

        // Copy the directory
        copy_dir_recursive(&stage_src, cache_path)?;

        // Make run script executable if it exists
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let run_path = cache_path.join("run");
            if run_path.exists() {
                let mut perms = std::fs::metadata(&run_path)
                    .context("Failed to get run script metadata")?
                    .permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&run_path, perms)
                    .context("Failed to make run script executable")?;
                debug!("Made run script executable: {}", run_path.display());
            }
        }

        info!("Cached action directory at: {}", cache_path.display());
        Ok(())
    }

    /// Fallback: fetch only step.yml (or legacy action.yml/stage.yaml) using raw content URL.
    async fn fetch_stage_yaml_only(
        &self,
        reference: &StageReference,
        cache_path: &PathBuf,
        git_ref: &str,
    ) -> Result<()> {
        // Create cache directory
        std::fs::create_dir_all(cache_path).with_context(|| {
            format!("Failed to create cache directory: {}", cache_path.display())
        })?;

        // Try step.yml first (preferred), then legacy fallbacks
        let filenames = ["step.yml", "action.yml", "stage.yaml"];
        let mut last_error = None;

        for filename in &filenames {
            let raw_url = format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}/{}",
                reference.owner, reference.repo, git_ref, reference.path, filename
            );

            debug!("Fetching {} from: {}", filename, raw_url);

            let response = match reqwest::get(&raw_url).await {
                Ok(r) => r,
                Err(e) => {
                    last_error = Some(format!("Failed to fetch {}: {}", filename, e));
                    continue;
                }
            };

            if response.status().is_success() {
                let content = response.text().await?;

                // Write to cache using the fetched filename
                let yaml_path = cache_path.join(filename);
                std::fs::write(&yaml_path, &content).with_context(|| {
                    format!("Failed to write {} to {}", filename, yaml_path.display())
                })?;

                info!("Cached {} at: {}", filename, cache_path.display());
                return Ok(());
            }

            last_error = Some(format!("HTTP {} for {}", response.status(), raw_url));
        }

        // Clean up empty cache directory
        let _ = std::fs::remove_dir(cache_path);
        Err(anyhow!(
            "Failed to fetch action definition from GitHub: {}",
            last_error.unwrap_or_else(|| "unknown error".to_string())
        ))
    }

    /// Resolve a version specifier to a git ref.
    async fn resolve_version_to_ref(&self, reference: &StageReference) -> Result<String> {
        match &reference.version {
            VersionSpec::SemverExact(v) => Ok(v.clone()),
            VersionSpec::Branch(b) => Ok(b.clone()),
            VersionSpec::Commit(c) => Ok(c.clone()),
            VersionSpec::SemverMajor(major) => {
                // For major version, we need to find the latest matching tag
                // For now, use a simple approach: try v{major}.0.0
                // In a full implementation, we would use the GitHub API to list tags
                warn!(
                    "SemverMajor resolution using v{}.0.0 - full tag resolution not yet implemented",
                    major
                );
                Ok(format!("v{}.0.0", major))
            }
        }
    }

    /// Read and parse step definition from a path (step.yml, action.yml, or stage.yaml).
    fn read_stage_yaml(&self, path: &PathBuf) -> Result<StageYaml> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read action definition from {}", path.display()))?;

        let stage_yaml: StageYaml = serde_yaml::from_str(&content).with_context(|| {
            format!("Failed to parse action definition from {}", path.display())
        })?;

        Ok(stage_yaml)
    }

    /// Clear the cache for a specific action reference.
    #[allow(dead_code)]
    pub fn clear_cache(&self, reference: &StageReference) -> Result<()> {
        let cache_path = self.cache_path(reference);
        if cache_path.exists() {
            std::fs::remove_dir_all(&cache_path)
                .with_context(|| format!("Failed to remove cache: {}", cache_path.display()))?;
        }
        Ok(())
    }
}

impl Default for StageLoader {
    fn default() -> Self {
        Self::new().expect("Failed to create StageLoader")
    }
}

/// Recursively copy a directory and its contents.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)
        .with_context(|| format!("Failed to create directory: {}", dst.display()))?;

    for entry in std::fs::read_dir(src)
        .with_context(|| format!("Failed to read directory: {}", src.display()))?
    {
        let entry = entry.context("Failed to read directory entry")?;
        let entry_path = entry.path();
        let dest_path = dst.join(entry.file_name());

        if entry_path.is_dir() {
            copy_dir_recursive(&entry_path, &dest_path)?;
        } else {
            std::fs::copy(&entry_path, &dest_path).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    entry_path.display(),
                    dest_path.display()
                )
            })?;
        }
    }

    Ok(())
}

/// Resolve relative paths in a run command to absolute paths.
///
/// If the run command starts with "./" or is just a filename without path separators,
/// it's treated as relative to the action directory and converted to an absolute path.
fn resolve_run_path(run: &str, action_dir: &Path) -> String {
    if run.starts_with("./") {
        // Explicit relative path: ./run -> /absolute/path/to/action/run
        let relative = run.strip_prefix("./").unwrap();
        action_dir.join(relative).to_string_lossy().to_string()
    } else if !run.contains('/') && !run.contains(' ') {
        // Simple command without path or arguments that might be a local script
        // Check if it exists in the action directory
        let potential_path = action_dir.join(run);
        if potential_path.exists() {
            potential_path.to_string_lossy().to_string()
        } else {
            // Assume it's a system command
            run.to_string()
        }
    } else {
        // Absolute path or command with arguments - use as-is
        run.to_string()
    }
}

/// Parse an action reference string.
///
/// Format: `owner/repo/path@version`
///
/// Examples:
/// - `airlock-hq/airlock/defaults/lint@v1`
/// - `myorg/actions/lint/go@v1.2.3`
/// - `user/repo/tools/format@main`
pub fn parse_stage_reference(reference: &str) -> Result<StageReference> {
    // Split by @
    let (path_part, version_part) = reference
        .rsplit_once('@')
        .ok_or_else(|| anyhow!("Action reference must include version: {}", reference))?;

    // Parse owner/repo/path
    let parts: Vec<&str> = path_part.split('/').collect();
    if parts.len() < 3 {
        return Err(anyhow!(
            "Action reference must be owner/repo/path format: {}",
            reference
        ));
    }

    let owner = parts[0].to_string();
    let repo = parts[1].to_string();
    let path = parts[2..].join("/");

    // Parse version
    let version = parse_version_spec(version_part)?;

    Ok(StageReference {
        owner,
        repo,
        path,
        version,
    })
}

/// Parse a version specifier string.
fn parse_version_spec(version: &str) -> Result<VersionSpec> {
    // Check for semver-like versions (v1, v1.2.3)
    if let Some(rest) = version.strip_prefix('v') {
        if rest.contains('.') {
            // Exact semver: v1.2.3
            return Ok(VersionSpec::SemverExact(version.to_string()));
        } else if let Ok(major) = rest.parse::<u32>() {
            // Major version: v1
            return Ok(VersionSpec::SemverMajor(major));
        }
    }

    // Check if it looks like a commit SHA (7+ hex characters)
    if version.len() >= 7 && version.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(VersionSpec::Commit(version.to_string()));
    }

    // Otherwise treat as branch name
    Ok(VersionSpec::Branch(version.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_action_reference_basic() {
        let ref_str = "airlock-hq/airlock/defaults/lint@v1";
        let reference = parse_stage_reference(ref_str).unwrap();

        assert_eq!(reference.owner, "airlock-hq");
        assert_eq!(reference.repo, "airlock");
        assert_eq!(reference.path, "defaults/lint");
        assert_eq!(reference.version, VersionSpec::SemverMajor(1));
    }

    #[test]
    fn test_parse_action_reference_exact_version() {
        let ref_str = "owner/repo/path/to/action@v1.2.3";
        let reference = parse_stage_reference(ref_str).unwrap();

        assert_eq!(reference.owner, "owner");
        assert_eq!(reference.repo, "repo");
        assert_eq!(reference.path, "path/to/action");
        assert_eq!(
            reference.version,
            VersionSpec::SemverExact("v1.2.3".to_string())
        );
    }

    #[test]
    fn test_parse_action_reference_branch() {
        let ref_str = "owner/repo/action@main";
        let reference = parse_stage_reference(ref_str).unwrap();

        assert_eq!(reference.version, VersionSpec::Branch("main".to_string()));
    }

    #[test]
    fn test_parse_action_reference_commit() {
        let ref_str = "owner/repo/action@abc1234";
        let reference = parse_stage_reference(ref_str).unwrap();

        assert_eq!(
            reference.version,
            VersionSpec::Commit("abc1234".to_string())
        );
    }

    #[test]
    fn test_parse_action_reference_missing_version() {
        let result = parse_stage_reference("owner/repo/path");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_action_reference_invalid_path() {
        let result = parse_stage_reference("owner/repo@v1");
        assert!(result.is_err());
    }

    #[test]
    fn test_version_spec_display() {
        assert_eq!(VersionSpec::SemverMajor(1).to_string(), "v1");
        assert_eq!(
            VersionSpec::SemverExact("v1.2.3".to_string()).to_string(),
            "v1.2.3"
        );
        assert_eq!(VersionSpec::Branch("main".to_string()).to_string(), "main");
        assert_eq!(
            VersionSpec::Commit("abc1234".to_string()).to_string(),
            "abc1234"
        );
    }

    #[test]
    fn test_action_yml_parsing_kebab_case() {
        let yaml = r#"
run: npm run lint
shell: bash
continue-on-error: true
require-approval: true
description: Run ESLint
"#;
        let stage_yaml: StageYaml = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(stage_yaml.run, "npm run lint");
        assert_eq!(stage_yaml.shell, Some("bash".to_string()));
        assert!(stage_yaml.continue_on_error);
        assert_eq!(stage_yaml.require_approval, ApprovalMode::Always);
        assert_eq!(stage_yaml.description, Some("Run ESLint".to_string()));
    }

    #[test]
    fn test_action_yml_parsing_legacy_snake_case() {
        // Legacy stage.yaml files use snake_case — should still work via alias
        let yaml = r#"
run: npm run lint
shell: bash
continue_on_error: true
description: Run ESLint
"#;
        let stage_yaml: StageYaml = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(stage_yaml.run, "npm run lint");
        assert_eq!(stage_yaml.shell, Some("bash".to_string()));
        assert!(stage_yaml.continue_on_error);
        assert_eq!(stage_yaml.require_approval, ApprovalMode::Never);
        assert_eq!(stage_yaml.description, Some("Run ESLint".to_string()));
    }

    #[test]
    fn test_action_yml_defaults() {
        let yaml = "run: echo hello";
        let stage_yaml: StageYaml = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(stage_yaml.run, "echo hello");
        assert_eq!(stage_yaml.shell, None);
        assert!(!stage_yaml.continue_on_error);
        assert_eq!(stage_yaml.require_approval, ApprovalMode::Never);
    }

    #[test]
    fn test_action_yml_parsing_env() {
        let yaml = r#"
run: echo "$MODE"
env:
  MODE: safe
"#;
        let stage_yaml: StageYaml = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(stage_yaml.env.get("MODE"), Some(&"safe".to_string()));
    }

    #[test]
    fn test_action_yml_parsing_if_patches() {
        let yaml = r#"
run: airlock exec push
require-approval: if_patches
"#;
        let stage_yaml: StageYaml = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(stage_yaml.require_approval, ApprovalMode::IfPatches);
    }

    #[test]
    fn test_cache_path_calculation() {
        let temp_dir = TempDir::new().unwrap();
        let loader = StageLoader::with_cache_dir(temp_dir.path().to_path_buf());

        let reference = StageReference {
            owner: "owner".to_string(),
            repo: "repo".to_string(),
            path: "defaults/lint".to_string(),
            version: VersionSpec::SemverMajor(1),
        };

        let cache_path = loader.cache_path(&reference);
        assert!(cache_path.to_string_lossy().contains("owner"));
        assert!(cache_path.to_string_lossy().contains("repo"));
        assert!(cache_path.to_string_lossy().contains("defaults_lint@v1"));
    }

    #[test]
    fn test_read_step_yml() {
        let temp_dir = TempDir::new().unwrap();
        let loader = StageLoader::with_cache_dir(temp_dir.path().to_path_buf());

        let yaml_content = r#"
run: cargo test
shell: bash
"#;
        let yaml_path = temp_dir.path().join("step.yml");
        std::fs::write(&yaml_path, yaml_content).unwrap();

        let stage_yaml = loader.read_stage_yaml(&yaml_path).unwrap();
        assert_eq!(stage_yaml.run, "cargo test");
        assert_eq!(stage_yaml.shell, Some("bash".to_string()));
    }

    #[tokio::test]
    async fn test_resolve_step_with_step_yml_cache() {
        let temp_dir = TempDir::new().unwrap();
        let loader = StageLoader::with_cache_dir(temp_dir.path().to_path_buf());

        // Create a cached step with step.yml (preferred format)
        let cache_dir = temp_dir
            .path()
            .join("owner")
            .join("repo")
            .join("defaults_lint@v1");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let yaml_content = r#"
run: npm run lint
shell: bash
continue-on-error: true
"#;
        std::fs::write(cache_dir.join("step.yml"), yaml_content).unwrap();

        // Create step definition that uses the cached step
        let stage = StepDefinition {
            name: "lint".to_string(),
            run: None,
            uses: Some("owner/repo/defaults/lint@v1".to_string()),
            shell: None,
            env: Default::default(),
            continue_on_error: false,
            require_approval: ApprovalMode::Never,
            timeout: None,
            apply_patch: false,
        };

        let resolved = loader.resolve_stage(&stage).await.unwrap();

        assert_eq!(resolved.name, "lint");
        assert_eq!(resolved.run, Some("npm run lint".to_string()));
        assert_eq!(resolved.shell, Some("bash".to_string()));
        // continue_on_error is OR'd: step.continue_on_error || step.yml.continue_on_error
        assert!(resolved.continue_on_error);
    }

    #[tokio::test]
    async fn test_resolve_step_with_legacy_stage_yaml_cache() {
        let temp_dir = TempDir::new().unwrap();
        let loader = StageLoader::with_cache_dir(temp_dir.path().to_path_buf());

        // Create a cached step with legacy stage.yaml (fallback)
        let cache_dir = temp_dir
            .path()
            .join("owner")
            .join("repo")
            .join("defaults_lint@v1");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let yaml_content = r#"
run: npm run lint
shell: bash
continue_on_error: true
"#;
        std::fs::write(cache_dir.join("stage.yaml"), yaml_content).unwrap();

        let stage = StepDefinition {
            name: "lint".to_string(),
            run: None,
            uses: Some("owner/repo/defaults/lint@v1".to_string()),
            shell: None,
            env: Default::default(),
            continue_on_error: false,
            require_approval: ApprovalMode::Never,
            timeout: None,
            apply_patch: false,
        };

        let resolved = loader.resolve_stage(&stage).await.unwrap();

        assert_eq!(resolved.name, "lint");
        assert_eq!(resolved.run, Some("npm run lint".to_string()));
        assert_eq!(resolved.shell, Some("bash".to_string()));
        assert!(resolved.continue_on_error);
    }

    #[tokio::test]
    async fn test_resolve_step_prefers_step_yml_over_legacy() {
        let temp_dir = TempDir::new().unwrap();
        let loader = StageLoader::with_cache_dir(temp_dir.path().to_path_buf());

        // Create a cached step with BOTH step.yml and stage.yaml
        let cache_dir = temp_dir
            .path()
            .join("owner")
            .join("repo")
            .join("defaults_lint@v1");
        std::fs::create_dir_all(&cache_dir).unwrap();

        // step.yml has bash shell (should be used)
        std::fs::write(
            cache_dir.join("step.yml"),
            "run: npm run lint\nshell: bash\n",
        )
        .unwrap();
        // stage.yaml has zsh shell (should NOT be used)
        std::fs::write(
            cache_dir.join("stage.yaml"),
            "run: npm run lint\nshell: zsh\n",
        )
        .unwrap();

        let stage = StepDefinition {
            name: "lint".to_string(),
            run: None,
            uses: Some("owner/repo/defaults/lint@v1".to_string()),
            shell: None,
            env: Default::default(),
            continue_on_error: false,
            require_approval: ApprovalMode::Never,
            timeout: None,
            apply_patch: false,
        };

        let resolved = loader.resolve_stage(&stage).await.unwrap();

        // Should use step.yml (bash), not stage.yaml (zsh)
        assert_eq!(resolved.shell, Some("bash".to_string()));
    }

    #[tokio::test]
    async fn test_resolve_step_inline_override() {
        let temp_dir = TempDir::new().unwrap();
        let loader = StageLoader::with_cache_dir(temp_dir.path().to_path_buf());

        // Create a cached step
        let cache_dir = temp_dir
            .path()
            .join("owner")
            .join("repo")
            .join("defaults_lint@v1");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let yaml_content = r#"
run: npm run lint
shell: bash
"#;
        std::fs::write(cache_dir.join("step.yml"), yaml_content).unwrap();

        // Create step definition with inline run override
        let stage = StepDefinition {
            name: "lint".to_string(),
            run: Some("npm run lint:strict".to_string()), // Override
            uses: Some("owner/repo/defaults/lint@v1".to_string()),
            shell: Some("zsh".to_string()), // Override shell
            env: Default::default(),
            continue_on_error: false,
            require_approval: ApprovalMode::Never,
            timeout: None,
            apply_patch: false,
        };

        let resolved = loader.resolve_stage(&stage).await.unwrap();

        // Inline properties should override
        assert_eq!(resolved.run, Some("npm run lint:strict".to_string()));
        assert_eq!(resolved.shell, Some("zsh".to_string()));
    }

    #[tokio::test]
    async fn test_resolve_step_merges_env() {
        let temp_dir = TempDir::new().unwrap();
        let loader = StageLoader::with_cache_dir(temp_dir.path().to_path_buf());

        let cache_dir = temp_dir
            .path()
            .join("owner")
            .join("repo")
            .join("defaults_gate@v1");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let yaml_content = r#"
run: echo "$MODE"
env:
  MODE: safe
"#;
        std::fs::write(cache_dir.join("step.yml"), yaml_content).unwrap();

        let stage = StepDefinition {
            name: "gate".to_string(),
            run: None,
            uses: Some("owner/repo/defaults/gate@v1".to_string()),
            env: [("MODE".to_string(), "strict".to_string())]
                .into_iter()
                .collect(),
            shell: None,
            continue_on_error: false,
            require_approval: ApprovalMode::Never,
            timeout: None,
            apply_patch: false,
        };

        let resolved = loader.resolve_stage(&stage).await.unwrap();
        assert_eq!(resolved.run, Some("echo \"$MODE\"".to_string()));
        assert_eq!(resolved.env.get("MODE"), Some(&"strict".to_string()));
    }

    // --- Bundled defaults tests ---

    #[test]
    fn test_get_bundled_default_matches_all_defaults() {
        let defaults = [
            "defaults/rebase",
            "defaults/lint",
            "defaults/document",
            "defaults/test",
            "defaults/describe",
            "defaults/push",
            "defaults/critique",
            "defaults/gate",
            "defaults/create-pr",
        ];
        for path in &defaults {
            let reference = StageReference {
                owner: "airlock-hq".to_string(),
                repo: "airlock".to_string(),
                path: path.to_string(),
                version: VersionSpec::Branch("main".to_string()),
            };
            assert!(
                get_bundled_default(&reference).is_some(),
                "Expected bundled default for {}",
                path
            );
        }
    }

    #[test]
    fn test_get_bundled_default_returns_none_for_non_main_branch() {
        let reference = StageReference {
            owner: "airlock-hq".to_string(),
            repo: "airlock".to_string(),
            path: "defaults/lint".to_string(),
            version: VersionSpec::Branch("develop".to_string()),
        };
        assert!(get_bundled_default(&reference).is_none());
    }

    #[test]
    fn test_get_bundled_default_returns_none_for_wrong_owner() {
        let reference = StageReference {
            owner: "someone-else".to_string(),
            repo: "airlock".to_string(),
            path: "defaults/lint".to_string(),
            version: VersionSpec::Branch("main".to_string()),
        };
        assert!(get_bundled_default(&reference).is_none());
    }

    #[test]
    fn test_get_bundled_default_returns_none_for_unknown_step() {
        let reference = StageReference {
            owner: "airlock-hq".to_string(),
            repo: "airlock".to_string(),
            path: "defaults/unknown-step".to_string(),
            version: VersionSpec::Branch("main".to_string()),
        };
        assert!(get_bundled_default(&reference).is_none());
    }

    #[test]
    fn test_bundled_defaults_are_valid_yaml() {
        let defaults = [
            BUNDLED_REBASE,
            BUNDLED_LINT,
            BUNDLED_DOCUMENT,
            BUNDLED_TEST,
            BUNDLED_DESCRIBE,
            BUNDLED_PUSH,
            BUNDLED_CRITIQUE,
            BUNDLED_GATE,
            BUNDLED_CREATE_PR,
        ];
        for yaml_str in &defaults {
            let parsed: Result<StageYaml, _> = serde_yaml::from_str(yaml_str);
            assert!(parsed.is_ok(), "Failed to parse bundled YAML: {:?}", parsed);
        }
    }

    #[test]
    fn test_bundled_gate_supports_never_threshold() {
        let gate: StageYaml = serde_yaml::from_str(BUNDLED_GATE).unwrap();
        assert_eq!(
            gate.env.get("AIRLOCK_RISK_THRESHOLD"),
            Some(&"medium".to_string())
        );
        assert!(gate.run.contains("never)"));
    }

    #[tokio::test]
    async fn test_resolve_stage_uses_bundled_default_no_cache() {
        let temp_dir = TempDir::new().unwrap();
        let loader = StageLoader::with_cache_dir(temp_dir.path().to_path_buf());

        // No cache directory created — bundled path should work without it
        let stage = StepDefinition {
            name: "lint".to_string(),
            run: None,
            uses: Some("airlock-hq/airlock/defaults/lint@main".to_string()),
            shell: None,
            env: Default::default(),
            continue_on_error: false,
            require_approval: ApprovalMode::Never,
            timeout: None,
            apply_patch: false,
        };

        let resolved = loader.resolve_stage(&stage).await.unwrap();

        assert_eq!(resolved.name, "lint");
        assert!(resolved.run.is_some());
        // Should resolve from bundled default, shell should come from the bundled YAML
        assert_eq!(resolved.shell, Some("bash".to_string()));
        // Cache directory should NOT have been created
        let cache_path = loader
            .cache_path(&parse_stage_reference("airlock-hq/airlock/defaults/lint@main").unwrap());
        assert!(!cache_path.exists());
    }

    // --- is_cache_stale tests ---

    #[test]
    fn test_is_cache_stale_fresh_dir() {
        let temp_dir = TempDir::new().unwrap();
        // Just-created dir should not be stale with a 1-hour TTL
        assert!(!is_cache_stale(temp_dir.path(), 3600));
    }

    #[test]
    fn test_is_cache_stale_old_dir() {
        let temp_dir = TempDir::new().unwrap();
        // Set mtime to 2 hours ago
        let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 3600);
        filetime::set_file_mtime(
            temp_dir.path(),
            filetime::FileTime::from_system_time(two_hours_ago),
        )
        .unwrap();
        // Should be stale with a 1-hour TTL
        assert!(is_cache_stale(temp_dir.path(), 3600));
    }

    #[test]
    fn test_is_cache_stale_nonexistent_path() {
        assert!(is_cache_stale(Path::new("/nonexistent/path/xyz"), 3600));
    }

    // --- TTL integration tests ---

    #[tokio::test]
    async fn test_stale_mutable_ref_triggers_refetch() {
        let temp_dir = TempDir::new().unwrap();
        let loader = StageLoader::with_cache_dir(temp_dir.path().to_path_buf());

        // Create a cached step for a branch ref
        let cache_dir = temp_dir
            .path()
            .join("owner")
            .join("repo")
            .join("my_action@main");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("step.yml"), "run: echo old\nshell: bash\n").unwrap();

        // Set cache mtime to 2 hours ago (stale)
        let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 3600);
        filetime::set_file_mtime(
            &cache_dir,
            filetime::FileTime::from_system_time(two_hours_ago),
        )
        .unwrap();

        let reference = StageReference {
            owner: "owner".to_string(),
            repo: "repo".to_string(),
            path: "my/action".to_string(),
            version: VersionSpec::Branch("main".to_string()),
        };

        // get_or_fetch_stage will remove the stale cache and try to fetch from GitHub,
        // which will fail in tests (no network). That's expected — the important thing
        // is that the stale cache was removed, triggering a re-fetch attempt.
        let result = loader.get_or_fetch_stage(&reference).await;
        assert!(
            result.is_err(),
            "Should fail because GitHub fetch fails in tests"
        );
        // Verify the stale cache was removed
        assert!(!cache_dir.exists());
    }

    #[tokio::test]
    async fn test_immutable_ref_ignores_ttl() {
        let temp_dir = TempDir::new().unwrap();
        let loader = StageLoader::with_cache_dir(temp_dir.path().to_path_buf());

        // Create a cached step for an exact semver ref
        let cache_dir = temp_dir
            .path()
            .join("owner")
            .join("repo")
            .join("my_action@v1.2.3");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(
            cache_dir.join("step.yml"),
            "run: echo pinned\nshell: bash\n",
        )
        .unwrap();

        // Set cache mtime to 30 days ago (very old)
        let old_time =
            std::time::SystemTime::now() - std::time::Duration::from_secs(30 * 24 * 3600);
        filetime::set_file_mtime(&cache_dir, filetime::FileTime::from_system_time(old_time))
            .unwrap();

        let reference = StageReference {
            owner: "owner".to_string(),
            repo: "repo".to_string(),
            path: "my/action".to_string(),
            version: VersionSpec::SemverExact("v1.2.3".to_string()),
        };

        // Should return the cached path without attempting re-fetch
        let result = loader.get_or_fetch_stage(&reference).await.unwrap();
        assert_eq!(result, cache_dir);
    }

    #[tokio::test]
    async fn test_fresh_mutable_ref_uses_cache() {
        let temp_dir = TempDir::new().unwrap();
        let loader = StageLoader::with_cache_dir(temp_dir.path().to_path_buf());

        // Create a fresh cached step for a branch ref
        let cache_dir = temp_dir
            .path()
            .join("owner")
            .join("repo")
            .join("my_action@develop");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("step.yml"), "run: echo fresh\nshell: bash\n").unwrap();

        // mtime is "now" (just created) — well within the 1-hour TTL

        let reference = StageReference {
            owner: "owner".to_string(),
            repo: "repo".to_string(),
            path: "my/action".to_string(),
            version: VersionSpec::Branch("develop".to_string()),
        };

        // Should return the cached path without re-fetching
        let result = loader.get_or_fetch_stage(&reference).await.unwrap();
        assert_eq!(result, cache_dir);
    }
}
