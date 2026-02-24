//! Stage executor for the stage-based pipeline.
//!
//! This module implements the core stage execution logic for the new architecture:
//! - Environment variable setup for each stage
//! - Shell command execution with configurable shell
//! - Capturing stdout, stderr, exit code, and duration
//! - Artifact directory management
//! - Support for `continue_on_error` and `require_approval` flags
//!
//! Each stage runs as a shell command in the run's worktree directory.

use airlock_core::{AirlockPaths, StepDefinition, StepResult, StepStatus};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// Callback for streaming log output.
/// Receives (stream_type: "stdout"|"stderr", content: String)
pub type LogStreamCallback = Arc<dyn Fn(&str, String) + Send + Sync>;

/// Maximum size (in bytes) for a single log file. Writes beyond this are dropped.
const MAX_LOG_FILE_SIZE: u64 = 50 * 1024 * 1024; // 50 MB

/// Append `content` to a log file, respecting the size cap.
/// Once the file exceeds the cap, a one-time truncation marker is written
/// and all further writes are silently dropped.
pub fn append_log_capped(path: &Path, content: &[u8]) {
    use std::io::Write;

    let current_size = match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(_) => 0, // File doesn't exist yet; will be created below
    };

    if current_size >= MAX_LOG_FILE_SIZE {
        return;
    }

    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(f) => f,
        Err(e) => {
            warn!("Failed to open log file {}: {}", path.display(), e);
            return;
        }
    };

    if current_size + content.len() as u64 > MAX_LOG_FILE_SIZE {
        let _ = file.write_all(b"\n[log truncated at 50 MB]\n");
        return;
    }

    if let Err(e) = file.write_all(content) {
        warn!("Failed to write to log file {}: {}", path.display(), e);
    }
}

// ---------------------------------------------------------------------------
// User shell & PATH resolution
// ---------------------------------------------------------------------------

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
fn get_user_login_shell() -> &'static str {
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

/// Resolve the user's full PATH by spawning their login shell.
///
/// Captures the PATH as the user would see it in a terminal session,
/// including additions from shell profiles (Homebrew, nvm, rustup, etc.).
/// The result is cached for the process lifetime.
fn resolve_user_path() -> &'static str {
    USER_PATH.get_or_init(|| {
        let shell = get_user_login_shell();

        debug!("Resolving user PATH via '{} -l -c echo $PATH'", shell);

        let result = std::process::Command::new(shell)
            .args(["-l", "-c", "echo $PATH"])
            .output();

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

/// Environment variables provided to each stage.
pub struct StageEnvironment {
    /// Unique run identifier (UUID).
    pub run_id: String,
    /// Branch being pushed (e.g., "refs/heads/feature/add-auth").
    pub branch: String,
    /// Base commit SHA (before push).
    pub base_sha: String,
    /// Head commit SHA (after push).
    pub head_sha: String,
    /// Absolute path to run worktree (also CWD).
    pub worktree: PathBuf,
    /// Directory for run-level artifacts (shared by all stages).
    pub artifacts: PathBuf,
    /// Directory for this stage's log files (stdout.log, stderr.log).
    pub logs_dir: PathBuf,
    /// Path to write JSON result (optional).
    pub stage_result_path: PathBuf,
    /// Path to the original working repository.
    pub repo_root: PathBuf,
    /// URL of the upstream remote.
    pub upstream_url: String,
    /// Path to the gate bare repository.
    pub gate_path: PathBuf,
    /// Default branch of the upstream repository (e.g., "main" or "master").
    pub default_branch: String,
    /// Job key (from workflow jobs map).
    pub job_key: Option<String>,
    /// Job display name.
    pub job_name: Option<String>,
    /// Git author name from the working repo (user.name config).
    pub git_author_name: Option<String>,
    /// Git author email from the working repo (user.email config).
    pub git_author_email: Option<String>,
}

impl StageEnvironment {
    /// Create environment variable map for the stage execution.
    pub fn to_env_vars(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert("AIRLOCK_RUN_ID".to_string(), self.run_id.clone());
        env.insert("AIRLOCK_BRANCH".to_string(), self.branch.clone());
        env.insert("AIRLOCK_BASE_SHA".to_string(), self.base_sha.clone());
        env.insert("AIRLOCK_HEAD_SHA".to_string(), self.head_sha.clone());
        env.insert(
            "AIRLOCK_WORKTREE".to_string(),
            self.worktree.to_string_lossy().to_string(),
        );
        env.insert(
            "AIRLOCK_ARTIFACTS".to_string(),
            self.artifacts.to_string_lossy().to_string(),
        );
        env.insert(
            "AIRLOCK_STAGE_RESULT".to_string(),
            self.stage_result_path.to_string_lossy().to_string(),
        );
        env.insert(
            "AIRLOCK_REPO_ROOT".to_string(),
            self.repo_root.to_string_lossy().to_string(),
        );
        env.insert(
            "AIRLOCK_UPSTREAM_URL".to_string(),
            self.upstream_url.clone(),
        );
        env.insert(
            "AIRLOCK_GATE_PATH".to_string(),
            self.gate_path.to_string_lossy().to_string(),
        );
        env.insert(
            "AIRLOCK_DEFAULT_BRANCH".to_string(),
            self.default_branch.clone(),
        );
        if let Some(ref job_key) = self.job_key {
            env.insert("AIRLOCK_JOB_KEY".to_string(), job_key.clone());
        }
        if let Some(ref job_name) = self.job_name {
            env.insert("AIRLOCK_JOB_NAME".to_string(), job_name.clone());
        }

        // Set git identity env vars so commits created by stages
        // are attributed to the user (author) with Airlock provenance (committer).
        if let Some(ref name) = self.git_author_name {
            env.insert("GIT_AUTHOR_NAME".to_string(), name.clone());
        }
        if let Some(ref email) = self.git_author_email {
            env.insert("GIT_AUTHOR_EMAIL".to_string(), email.clone());
        }
        env.insert("GIT_COMMITTER_NAME".to_string(), "Airlock".to_string());
        env.insert(
            "GIT_COMMITTER_EMAIL".to_string(),
            "airlock@airlockhq.com".to_string(),
        );

        // Build PATH for child processes:
        // 1. Prepend the daemon binary dir (so steps can find `airlock` CLI)
        // 2. Use the resolved user PATH as the base (instead of the daemon's
        //    bare launchd/systemd PATH) so user-installed tools like `claude`,
        //    `node`, `cargo`, etc. are available.
        if let Ok(exe) = std::env::current_exe() {
            if let Some(bin_dir) = exe.parent() {
                let base_path = resolve_user_path();
                let new_path = format!("{}:{}", bin_dir.display(), base_path);
                env.insert("PATH".to_string(), new_path);
            }
        }

        env
    }
}

/// Result of executing a single stage.
#[derive(Debug, Clone)]
pub struct StageExecutionResult {
    /// Exit code of the command.
    pub exit_code: i32,
    /// Standard output captured.
    pub stdout: String,
    /// Standard error captured.
    pub stderr: String,
    /// Duration of the execution in milliseconds.
    pub duration_ms: i64,
    /// Whether the stage passed (exit_code == 0).
    pub passed: bool,
}

/// Create the stage logs directory.
///
/// Directory structure:
/// - With job_key: `~/.airlock/artifacts/<repo-id>/<run-id>/logs/<job_key>/<stage-name>/`
/// - Without job_key: `~/.airlock/artifacts/<repo-id>/<run-id>/logs/<stage-name>/`
pub fn create_stage_logs_dir(
    paths: &AirlockPaths,
    repo_id: &str,
    run_id: &str,
    stage_name: &str,
    job_key: Option<&str>,
) -> Result<PathBuf> {
    let mut logs_dir = paths.run_artifacts(repo_id, run_id).join("logs");
    if let Some(jk) = job_key {
        logs_dir = logs_dir.join(jk);
    }
    logs_dir = logs_dir.join(stage_name);

    std::fs::create_dir_all(&logs_dir)
        .with_context(|| format!("Failed to create stage logs directory: {:?}", logs_dir))?;

    debug!("Created stage logs directory: {:?}", logs_dir);
    Ok(logs_dir)
}

/// Create the run artifacts directory.
///
/// Directory structure:
/// ```text
/// ~/.airlock/artifacts/<repo-id>/<run-id>/
/// ├── logs/              # Stage log files (stdout.log, stderr.log per stage)
/// ├── description.json   # From describe stage
/// ├── pr_result.json     # From create-pr stage
/// └── ...
/// ```
pub fn create_run_artifacts_dir(
    paths: &AirlockPaths,
    repo_id: &str,
    run_id: &str,
) -> Result<PathBuf> {
    let run_artifacts_dir = paths.run_artifacts(repo_id, run_id);
    let logs_dir = run_artifacts_dir.join("logs");

    std::fs::create_dir_all(&run_artifacts_dir).with_context(|| {
        format!(
            "Failed to create run artifacts directory: {:?}",
            run_artifacts_dir
        )
    })?;
    std::fs::create_dir_all(&logs_dir)
        .with_context(|| format!("Failed to create logs directory: {:?}", logs_dir))?;

    debug!("Created run artifacts directory: {:?}", run_artifacts_dir);
    Ok(run_artifacts_dir)
}

/// Parameters for building a stage environment.
pub struct StageEnvironmentParams<'a> {
    /// Airlock paths.
    pub paths: &'a AirlockPaths,
    /// Repository ID.
    pub repo_id: &'a str,
    /// Run ID.
    pub run_id: &'a str,
    /// Stage name.
    pub stage_name: &'a str,
    /// Branch being pushed.
    pub branch: &'a str,
    /// Base commit SHA.
    pub base_sha: &'a str,
    /// Head commit SHA.
    pub head_sha: &'a str,
    /// Path to the worktree.
    pub worktree_path: &'a Path,
    /// Path to the original repository root.
    pub repo_root: &'a Path,
    /// URL of the upstream remote.
    pub upstream_url: &'a str,
    /// Path to the gate bare repo (used for default branch detection).
    pub gate_path: &'a Path,
    /// Job key (from workflow jobs map). When provided, logs are written under `logs/{job_key}/{stage_name}/`.
    pub job_key: Option<&'a str>,
}

/// Detect the default branch of the upstream repository from the gate bare repo.
///
/// Tries in order:
/// 1. `git symbolic-ref refs/remotes/origin/HEAD` (set by `git clone` or `git remote set-head`)
/// 2. Check if `origin/main` ref exists
/// 3. Check if `origin/master` ref exists
/// 4. Fall back to `"main"`
pub fn detect_default_branch(gate_path: &Path) -> String {
    // Try symbolic-ref first (most reliable if set)
    if let Ok(output) = std::process::Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .current_dir(gate_path)
        .output()
    {
        if output.status.success() {
            let refname = String::from_utf8_lossy(&output.stdout).trim().to_string();
            // refname is like "refs/remotes/origin/main"
            if let Some(branch) = refname.strip_prefix("refs/remotes/origin/") {
                debug!("Detected default branch via symbolic-ref: {}", branch);
                return branch.to_string();
            }
        }
    }

    // Fall back: check if origin/main exists
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "refs/remotes/origin/main"])
        .current_dir(gate_path)
        .output()
    {
        if output.status.success() {
            debug!("Detected default branch via origin/main ref");
            return "main".to_string();
        }
    }

    // Fall back: check if origin/master exists
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--verify", "refs/remotes/origin/master"])
        .current_dir(gate_path)
        .output()
    {
        if output.status.success() {
            debug!("Detected default branch via origin/master ref");
            return "master".to_string();
        }
    }

    // Final fallback
    debug!("Could not detect default branch, falling back to 'main'");
    "main".to_string()
}

/// Build the StageEnvironment for a stage execution.
pub fn build_stage_environment(params: &StageEnvironmentParams<'_>) -> Result<StageEnvironment> {
    // Create stage logs directory
    let logs_dir = create_stage_logs_dir(
        params.paths,
        params.repo_id,
        params.run_id,
        params.stage_name,
        params.job_key,
    )?;
    let run_artifacts_dir = params.paths.run_artifacts(params.repo_id, params.run_id);
    let stage_result_path = logs_dir.join("result.json");

    let default_branch = detect_default_branch(params.gate_path);

    // Read git author identity from the working repo
    let git_author_name = airlock_core::git::get_git_config(params.repo_root, "user.name");
    let git_author_email = airlock_core::git::get_git_config(params.repo_root, "user.email");

    Ok(StageEnvironment {
        run_id: params.run_id.to_string(),
        branch: params.branch.to_string(),
        base_sha: params.base_sha.to_string(),
        head_sha: params.head_sha.to_string(),
        worktree: params.worktree_path.to_path_buf(),
        artifacts: run_artifacts_dir,
        logs_dir,
        stage_result_path,
        repo_root: params.repo_root.to_path_buf(),
        upstream_url: params.upstream_url.to_string(),
        gate_path: params.gate_path.to_path_buf(),
        default_branch,
        job_key: params.job_key.map(|s| s.to_string()),
        job_name: None,
        git_author_name,
        git_author_email,
    })
}

/// Execute a stage command with optional log streaming and cancellation support.
///
/// Similar to `execute_stage_command` but accepts an optional callback for
/// streaming output in real-time. The callback receives (stream_type, content)
/// where stream_type is "stdout" or "stderr".
///
/// If `cancel` is provided and triggered, the child process is killed and
/// a cancellation result is returned.
pub async fn execute_stage_command_with_streaming(
    stage: &StepDefinition,
    env: &StageEnvironment,
    timeout: Duration,
    log_callback: Option<LogStreamCallback>,
    cancel: Option<&CancellationToken>,
) -> Result<StageExecutionResult> {
    // Get the run command - must be present (either directly or resolved from uses)
    let run_command = stage.run.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "Stage '{}' has no run command. If using 'use:', ensure the stage has been resolved.",
            stage.name
        )
    })?;

    info!("Executing stage '{}': {}", stage.name, run_command);

    // Determine shell to use.
    // All shells run with -l (login) so that shell profiles are sourced,
    // providing env vars beyond PATH (API keys, version managers, etc.).
    let (shell_cmd, shell_args) = match stage.shell.as_deref() {
        None => {
            // Default: use user's login shell (detected from system if $SHELL
            // is not set, which is typical for daemons).
            let user_shell = get_user_login_shell().to_string();
            (user_shell, vec!["-l".to_string(), "-c".to_string()])
        }
        Some(shell) => (shell.to_string(), vec!["-l".to_string(), "-c".to_string()]),
    };

    let start_time = Instant::now();

    // Build and spawn the command, falling back to sh if the login shell fails
    let spawn_shell = |shell: &str, args: &[String]| {
        let mut cmd = Command::new(shell);
        cmd.args(args)
            .arg(run_command)
            .current_dir(&env.worktree)
            .envs(env.to_env_vars())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    };

    debug!(
        "Running command in {:?}: {} {} '{}'",
        env.worktree,
        shell_cmd,
        shell_args.join(" "),
        run_command
    );

    // Spawn the process, with fallback for default (login) shell
    let mut child = match spawn_shell(&shell_cmd, &shell_args).spawn() {
        Ok(child) => child,
        Err(e) if stage.shell.is_none() => {
            // Login shell failed to spawn (e.g. $SHELL points to a missing binary).
            // Fall back to sh -c so the step still runs, albeit without the full
            // user environment.
            warn!(
                "Failed to spawn login shell '{}' for stage '{}': {}. Falling back to sh.",
                shell_cmd, stage.name, e
            );
            spawn_shell("sh", &["-c".to_string()])
                .spawn()
                .with_context(|| {
                    format!(
                        "Failed to spawn fallback shell 'sh' for stage '{}'",
                        stage.name
                    )
                })?
        }
        Err(e) => {
            return Err(e).with_context(|| {
                format!(
                    "Failed to spawn command for stage '{}': {} {} '{}'",
                    stage.name,
                    shell_cmd,
                    shell_args.join(" "),
                    run_command
                )
            });
        }
    };

    // Take stdout and stderr handles
    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();

    // Inner future that waits for the process with timeout, streaming output
    let process_future = async {
        // If we have a log callback, stream line by line with batching
        // Otherwise just read all at once
        let (stdout, stderr) = if let Some(callback) = log_callback {
            // Spawn tasks to read stdout and stderr concurrently
            let stdout_callback = callback.clone();
            let stderr_callback = callback;

            let stdout_task = tokio::spawn(async move {
                let mut stdout_content = String::new();
                if let Some(handle) = stdout_handle {
                    let mut reader = BufReader::new(handle);
                    let mut batch = Vec::new();
                    let mut last_emit = Instant::now();
                    let batch_interval = Duration::from_millis(100);

                    let mut line = String::new();
                    loop {
                        line.clear();
                        match reader.read_line(&mut line).await {
                            Ok(0) => break, // EOF
                            Ok(_) => {
                                stdout_content.push_str(&line);
                                batch.push(line.clone());

                                // Emit batch if interval passed or batch is large
                                if last_emit.elapsed() >= batch_interval || batch.len() >= 50 {
                                    let content = batch.join("");
                                    stdout_callback("stdout", content);
                                    batch.clear();
                                    last_emit = Instant::now();
                                }
                            }
                            Err(_) => break,
                        }
                    }

                    // Emit remaining content
                    if !batch.is_empty() {
                        let content = batch.join("");
                        stdout_callback("stdout", content);
                    }
                }
                stdout_content
            });

            let stderr_task = tokio::spawn(async move {
                let mut stderr_content = String::new();
                if let Some(handle) = stderr_handle {
                    let mut reader = BufReader::new(handle);
                    let mut batch = Vec::new();
                    let mut last_emit = Instant::now();
                    let batch_interval = Duration::from_millis(100);

                    let mut line = String::new();
                    loop {
                        line.clear();
                        match reader.read_line(&mut line).await {
                            Ok(0) => break, // EOF
                            Ok(_) => {
                                stderr_content.push_str(&line);
                                batch.push(line.clone());

                                // Emit batch if interval passed or batch is large
                                if last_emit.elapsed() >= batch_interval || batch.len() >= 50 {
                                    let content = batch.join("");
                                    stderr_callback("stderr", content);
                                    batch.clear();
                                    last_emit = Instant::now();
                                }
                            }
                            Err(_) => break,
                        }
                    }

                    // Emit remaining content
                    if !batch.is_empty() {
                        let content = batch.join("");
                        stderr_callback("stderr", content);
                    }
                }
                stderr_content
            });

            let stdout = stdout_task.await.unwrap_or_default();
            let stderr = stderr_task.await.unwrap_or_default();
            (stdout, stderr)
        } else {
            // Non-streaming mode - read all at once
            let mut stdout = String::new();
            if let Some(mut handle) = stdout_handle {
                handle.read_to_string(&mut stdout).await.ok();
            }

            let mut stderr = String::new();
            if let Some(mut handle) = stderr_handle {
                handle.read_to_string(&mut stderr).await.ok();
            }
            (stdout, stderr)
        };

        // Wait for the process to complete
        let status = child.wait().await?;

        Ok::<_, anyhow::Error>((status, stdout, stderr))
    };

    // Wrap in timeout + cancellation
    let result = if let Some(token) = cancel {
        tokio::select! {
            biased;
            _ = token.cancelled() => {
                // Cancellation requested — kill the child process
                warn!("Stage '{}' cancelled (superseded by newer push)", stage.name);
                child.kill().await.ok();
                let duration_ms = start_time.elapsed().as_millis() as i64;
                return Ok(StageExecutionResult {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: "Cancelled: superseded by newer push".to_string(),
                    duration_ms,
                    passed: false,
                });
            }
            r = tokio::time::timeout(timeout, process_future) => r,
        }
    } else {
        tokio::time::timeout(timeout, process_future).await
    };

    let duration = start_time.elapsed();
    let duration_ms = duration.as_millis() as i64;

    match result {
        Ok(Ok((status, stdout, stderr))) => {
            let exit_code = status.code().unwrap_or(-1);
            let passed = exit_code == 0;

            if passed {
                info!(
                    "Stage '{}' passed (exit code: {}, duration: {:?})",
                    stage.name, exit_code, duration
                );
            } else {
                warn!(
                    "Stage '{}' failed (exit code: {}, duration: {:?})",
                    stage.name, exit_code, duration
                );
            }

            debug!("Stage '{}' stdout:\n{}", stage.name, stdout);
            if !stderr.is_empty() {
                debug!("Stage '{}' stderr:\n{}", stage.name, stderr);
            }

            Ok(StageExecutionResult {
                exit_code,
                stdout,
                stderr,
                duration_ms,
                passed,
            })
        }
        Ok(Err(e)) => {
            warn!("Stage '{}' process error: {}", stage.name, e);
            Ok(StageExecutionResult {
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("Process error: {}", e),
                duration_ms,
                passed: false,
            })
        }
        Err(_) => {
            warn!("Stage '{}' timed out after {:?}", stage.name, timeout);
            // Kill the process
            child.kill().await.ok();

            Ok(StageExecutionResult {
                exit_code: -1,
                stdout: String::new(),
                stderr: format!("Stage timed out after {:?}", timeout),
                duration_ms,
                passed: false,
            })
        }
    }
}

/// Execute a stage with optional log streaming callback.
///
/// Like `execute_stage` but accepts a callback for streaming output in real-time.
/// The optional `cancel` token allows the stage to be cancelled mid-execution.
pub async fn execute_stage_with_log_callback(
    stage: &StepDefinition,
    stage_result_id: &str,
    run_id: &str,
    env: &StageEnvironment,
    timeout: Duration,
    log_callback: Option<LogStreamCallback>,
    cancel: Option<&CancellationToken>,
) -> Result<StepResult> {
    let started_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // Create initial step result (Running status)
    // Note: step_order is preserved from the original insert, this is just for the update
    let mut stage_result = StepResult {
        id: stage_result_id.to_string(),
        run_id: run_id.to_string(),
        job_id: String::new(), // Preserved from original insert
        name: stage.name.clone(),
        status: StepStatus::Running,
        step_order: 0, // Preserved from original insert, not updated
        exit_code: None,
        duration_ms: None,
        error: None,
        started_at: Some(started_at),
        completed_at: None,
    };

    // Execute the command with streaming if callback is provided
    let exec_result =
        execute_stage_command_with_streaming(stage, env, timeout, log_callback, cancel).await?;

    // Update stage result based on execution
    let completed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    stage_result.exit_code = Some(exec_result.exit_code);
    stage_result.duration_ms = Some(exec_result.duration_ms);
    stage_result.completed_at = Some(completed_at);

    // Determine final status
    if exec_result.passed {
        if stage.require_approval {
            // Stage passed but requires approval before proceeding
            stage_result.status = StepStatus::AwaitingApproval;
            info!("Stage '{}' completed and awaiting approval", stage.name);
        } else {
            stage_result.status = StepStatus::Passed;
        }
    } else {
        // Stage failed
        stage_result.status = StepStatus::Failed;
        stage_result.error = Some(format!(
            "Exit code: {}. {}",
            exec_result.exit_code,
            if !exec_result.stderr.is_empty() {
                exec_result
                    .stderr
                    .lines()
                    .take(10)
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                "No error output".to_string()
            }
        ));
    }

    // Write stdout/stderr to log files
    write_stage_logs(&env.logs_dir, &exec_result)?;

    Ok(stage_result)
}

/// Null SHA used by Git for new branches (no previous ref).
const NULL_SHA: &str = "0000000000000000000000000000000000000000";

/// Resolve the effective base SHA for a run.
///
/// This function handles three cases:
/// 1. **New branch**: Git reports base SHA as all zeros (null SHA) → compute merge-base
/// 2. **Force push**: base SHA exists but is not an ancestor of HEAD → compute merge-base
/// 3. **Normal push**: base SHA is a valid ancestor → use as-is
///
/// For cases 1 and 2, we find the merge-base with the default branch (main/master),
/// or fall back to a limited ancestor.
///
/// This must be called after the worktree has been created, since it runs
/// git commands inside the worktree.
pub fn resolve_effective_base_sha(worktree_path: &Path, base_sha: &str) -> Result<String> {
    // Case 1: Null SHA (new branch)
    if base_sha == NULL_SHA {
        debug!(
            "Null base SHA detected (new branch), resolving effective base in {:?}",
            worktree_path
        );
        return compute_merge_base_fallback(worktree_path, "new branch");
    }

    // Case 2: Check if base_sha is an ancestor of HEAD (detects force push)
    // If base_sha is NOT an ancestor, this is a force push and we need merge-base
    if !is_ancestor(worktree_path, base_sha) {
        debug!(
            "Base SHA {} is not an ancestor of HEAD (force push detected), resolving effective base in {:?}",
            &base_sha[..8.min(base_sha.len())],
            worktree_path
        );
        return compute_merge_base_fallback(worktree_path, "force push");
    }

    // Case 3: Normal push - base_sha is a valid ancestor
    Ok(base_sha.to_string())
}

/// Check if a commit is an ancestor of HEAD.
fn is_ancestor(worktree_path: &Path, commit_sha: &str) -> bool {
    let output = std::process::Command::new("git")
        .args(["merge-base", "--is-ancestor", commit_sha, "HEAD"])
        .current_dir(worktree_path)
        .output();

    match output {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// Compute the merge-base with default branch, with fallbacks.
///
/// Tries in order:
/// 1. merge-base with main
/// 2. merge-base with master
/// 3. HEAD~20 (limit scope for very old branches)
/// 4. Root commit (last resort)
fn compute_merge_base_fallback(worktree_path: &Path, reason: &str) -> Result<String> {
    // Try merge-base with main, then master
    for branch in &["main", "master"] {
        let output = std::process::Command::new("git")
            .args(["merge-base", "HEAD", branch])
            .current_dir(worktree_path)
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !sha.is_empty() {
                    info!(
                        "Resolved base SHA via merge-base with {} ({}): {}",
                        branch, reason, sha
                    );
                    return Ok(sha);
                }
            }
        }
    }

    // Fallback: HEAD~20 to limit scope
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD~20"])
        .current_dir(worktree_path)
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !sha.is_empty() {
                info!("Resolved base SHA via HEAD~20 ({}): {}", reason, sha);
                return Ok(sha);
            }
        }
    }

    // Last resort: root commit
    let output = std::process::Command::new("git")
        .args(["rev-list", "--max-parents=0", "HEAD"])
        .current_dir(worktree_path)
        .output()
        .context("Failed to find root commit")?;

    let sha = String::from_utf8_lossy(&output.stdout)
        .lines()
        .last()
        .unwrap_or("")
        .trim()
        .to_string();

    if sha.is_empty() {
        anyhow::bail!("Could not resolve effective base SHA: no commits found");
    }

    info!("Resolved base SHA via root commit ({}): {}", reason, sha);
    Ok(sha)
}

/// Write stage stdout/stderr to log files in the logs directory.
///
/// If log files already exist (written incrementally by the log streaming callback),
/// this is a no-op for those files to avoid overwriting.
fn write_stage_logs(logs_dir: &Path, result: &StageExecutionResult) -> Result<()> {
    let stdout_path = logs_dir.join("stdout.log");
    let stderr_path = logs_dir.join("stderr.log");

    // Only write if the file doesn't already exist (incremental writes handle the streaming case)
    if !result.stdout.is_empty() && !stdout_path.exists() {
        std::fs::write(&stdout_path, &result.stdout)
            .with_context(|| format!("Failed to write stdout to {:?}", stdout_path))?;
    }

    if !result.stderr.is_empty() && !stderr_path.exists() {
        std::fs::write(&stderr_path, &result.stderr)
            .with_context(|| format!("Failed to write stderr to {:?}", stderr_path))?;
    }

    Ok(())
}

/// Determine if the pipeline should continue after a stage result.
///
/// Returns `true` if the pipeline should continue, `false` if it should stop.
///
/// Rules:
/// - If stage passed or is awaiting approval: continue (unless awaiting, which pauses)
/// - If stage failed and `continue_on_error` is true: continue
/// - If stage failed and `continue_on_error` is false: stop
pub fn should_continue_pipeline(stage: &StepDefinition, result: &StepResult) -> bool {
    match result.status {
        StepStatus::Passed => true,
        StepStatus::AwaitingApproval => false, // Pipeline pauses, but doesn't fail
        StepStatus::Failed => stage.continue_on_error,
        StepStatus::Skipped => true,
        StepStatus::Pending | StepStatus::Running => {
            // These shouldn't happen after execution, but treat as continue
            true
        }
    }
}

/// Determine if the pipeline should pause for approval.
pub fn should_pause_for_approval(result: &StepResult) -> bool {
    result.status == StepStatus::AwaitingApproval
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_stage(name: &str, run: &str) -> StepDefinition {
        StepDefinition {
            name: name.to_string(),
            run: Some(run.to_string()),
            uses: None,
            shell: None,
            continue_on_error: false,
            require_approval: false,
            timeout: None,
        }
    }

    fn create_test_env(temp_dir: &TempDir) -> StageEnvironment {
        let worktree = temp_dir.path().join("worktree");
        let artifacts = temp_dir.path().join("artifacts");
        let logs_dir = temp_dir.path().join("logs");
        let stage_result_path = logs_dir.join("result.json");
        let repo_root = temp_dir.path().join("repo");
        let gate_path = temp_dir.path().join("gate.git");

        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::create_dir_all(&artifacts).unwrap();
        std::fs::create_dir_all(&logs_dir).unwrap();
        std::fs::create_dir_all(&repo_root).unwrap();
        std::fs::create_dir_all(&gate_path).unwrap();

        StageEnvironment {
            run_id: "run-123".to_string(),
            branch: "refs/heads/feature/test".to_string(),
            base_sha: "abc123".to_string(),
            head_sha: "def456".to_string(),
            worktree,
            artifacts,
            logs_dir,
            stage_result_path,
            repo_root,
            upstream_url: "git@github.com:user/repo.git".to_string(),
            gate_path,
            default_branch: "main".to_string(),
            job_key: None,
            job_name: None,
            git_author_name: Some("Test User".to_string()),
            git_author_email: Some("test@example.com".to_string()),
        }
    }

    #[test]
    fn test_stage_environment_to_env_vars() {
        let temp_dir = TempDir::new().unwrap();
        let env = create_test_env(&temp_dir);

        let vars = env.to_env_vars();

        assert_eq!(vars.get("AIRLOCK_RUN_ID"), Some(&"run-123".to_string()));
        assert_eq!(
            vars.get("AIRLOCK_BRANCH"),
            Some(&"refs/heads/feature/test".to_string())
        );
        assert_eq!(vars.get("AIRLOCK_BASE_SHA"), Some(&"abc123".to_string()));
        assert_eq!(vars.get("AIRLOCK_HEAD_SHA"), Some(&"def456".to_string()));
        assert!(vars.contains_key("AIRLOCK_WORKTREE"));
        assert!(vars.contains_key("AIRLOCK_ARTIFACTS"));
        assert!(vars.contains_key("AIRLOCK_STAGE_RESULT"));
        assert!(vars.contains_key("AIRLOCK_REPO_ROOT"));
        assert_eq!(
            vars.get("AIRLOCK_UPSTREAM_URL"),
            Some(&"git@github.com:user/repo.git".to_string())
        );
        assert!(vars.contains_key("AIRLOCK_GATE_PATH"));
        assert_eq!(
            vars.get("AIRLOCK_DEFAULT_BRANCH"),
            Some(&"main".to_string())
        );
        // AIRLOCK_RUN_ARTIFACTS is no longer set (unified with AIRLOCK_ARTIFACTS)
        assert!(!vars.contains_key("AIRLOCK_RUN_ARTIFACTS"));
    }

    #[test]
    fn test_stage_environment_sets_git_identity_env_vars() {
        let temp_dir = TempDir::new().unwrap();
        let env = create_test_env(&temp_dir);

        let vars = env.to_env_vars();

        // Author should come from working repo config
        assert_eq!(vars.get("GIT_AUTHOR_NAME"), Some(&"Test User".to_string()));
        assert_eq!(
            vars.get("GIT_AUTHOR_EMAIL"),
            Some(&"test@example.com".to_string())
        );

        // Committer should always be Airlock
        assert_eq!(vars.get("GIT_COMMITTER_NAME"), Some(&"Airlock".to_string()));
        assert_eq!(
            vars.get("GIT_COMMITTER_EMAIL"),
            Some(&"airlock@airlockhq.com".to_string())
        );
    }

    #[test]
    fn test_stage_environment_omits_author_when_none() {
        let temp_dir = TempDir::new().unwrap();
        let mut env = create_test_env(&temp_dir);
        env.git_author_name = None;
        env.git_author_email = None;

        let vars = env.to_env_vars();

        // Author env vars should not be set
        assert!(!vars.contains_key("GIT_AUTHOR_NAME"));
        assert!(!vars.contains_key("GIT_AUTHOR_EMAIL"));

        // Committer should always be set
        assert_eq!(vars.get("GIT_COMMITTER_NAME"), Some(&"Airlock".to_string()));
        assert_eq!(
            vars.get("GIT_COMMITTER_EMAIL"),
            Some(&"airlock@airlockhq.com".to_string())
        );
    }

    #[tokio::test]
    async fn test_execute_stage_command_success() {
        let temp_dir = TempDir::new().unwrap();
        let env = create_test_env(&temp_dir);

        let stage = create_test_stage("test", "echo 'hello world'");
        let result =
            execute_stage_command_with_streaming(&stage, &env, Duration::from_secs(10), None, None)
                .await
                .unwrap();

        assert!(result.passed);
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello world"));
    }

    #[tokio::test]
    async fn test_execute_stage_command_failure() {
        let temp_dir = TempDir::new().unwrap();
        let env = create_test_env(&temp_dir);

        let stage = create_test_stage("test", "exit 1");
        let result =
            execute_stage_command_with_streaming(&stage, &env, Duration::from_secs(10), None, None)
                .await
                .unwrap();

        assert!(!result.passed);
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_execute_stage_command_with_env_vars() {
        let temp_dir = TempDir::new().unwrap();
        let env = create_test_env(&temp_dir);

        let stage = create_test_stage("test", "echo $AIRLOCK_RUN_ID");
        let result =
            execute_stage_command_with_streaming(&stage, &env, Duration::from_secs(10), None, None)
                .await
                .unwrap();

        assert!(result.passed);
        assert!(result.stdout.contains("run-123"));
    }

    #[tokio::test]
    async fn test_execute_stage_command_timeout() {
        let temp_dir = TempDir::new().unwrap();
        let env = create_test_env(&temp_dir);

        let stage = create_test_stage("test", "sleep 10");
        let result = execute_stage_command_with_streaming(
            &stage,
            &env,
            Duration::from_millis(100),
            None,
            None,
        )
        .await
        .unwrap();

        assert!(!result.passed);
        assert_eq!(result.exit_code, -1);
        assert!(result.stderr.contains("timed out"));
    }

    #[tokio::test]
    async fn test_execute_stage_command_with_bash() {
        let temp_dir = TempDir::new().unwrap();
        let env = create_test_env(&temp_dir);

        let mut stage = create_test_stage("test", "echo 'using bash'");
        stage.shell = Some("bash".to_string());

        let result =
            execute_stage_command_with_streaming(&stage, &env, Duration::from_secs(10), None, None)
                .await
                .unwrap();

        assert!(result.passed);
        assert!(result.stdout.contains("using bash"));
    }

    #[test]
    fn test_should_continue_pipeline_passed() {
        let stage = create_test_stage("test", "true");
        let result = StepResult {
            id: "sr-1".to_string(),
            run_id: "run-1".to_string(),
            job_id: String::new(),
            name: "test".to_string(),
            status: StepStatus::Passed,
            step_order: 0,
            exit_code: Some(0),
            duration_ms: Some(100),
            error: None,
            started_at: None,
            completed_at: None,
        };

        assert!(should_continue_pipeline(&stage, &result));
    }

    #[test]
    fn test_should_continue_pipeline_failed_no_continue() {
        let stage = create_test_stage("test", "false");
        let result = StepResult {
            id: "sr-1".to_string(),
            run_id: "run-1".to_string(),
            job_id: String::new(),
            name: "test".to_string(),
            status: StepStatus::Failed,
            step_order: 0,
            exit_code: Some(1),
            duration_ms: Some(100),
            error: Some("Failed".to_string()),
            started_at: None,
            completed_at: None,
        };

        assert!(!should_continue_pipeline(&stage, &result));
    }

    #[test]
    fn test_should_continue_pipeline_failed_with_continue() {
        let mut stage = create_test_stage("test", "false");
        stage.continue_on_error = true;

        let result = StepResult {
            id: "sr-1".to_string(),
            run_id: "run-1".to_string(),
            job_id: String::new(),
            name: "test".to_string(),
            status: StepStatus::Failed,
            step_order: 0,
            exit_code: Some(1),
            duration_ms: Some(100),
            error: Some("Failed".to_string()),
            started_at: None,
            completed_at: None,
        };

        assert!(should_continue_pipeline(&stage, &result));
    }

    #[test]
    fn test_should_continue_pipeline_awaiting_approval() {
        let stage = create_test_stage("review", "true");
        let result = StepResult {
            id: "sr-1".to_string(),
            run_id: "run-1".to_string(),
            job_id: String::new(),
            name: "review".to_string(),
            status: StepStatus::AwaitingApproval,
            step_order: 0,
            exit_code: Some(0),
            duration_ms: Some(100),
            error: None,
            started_at: None,
            completed_at: None,
        };

        // Pipeline should pause (not continue) when awaiting approval
        assert!(!should_continue_pipeline(&stage, &result));
    }

    #[test]
    fn test_should_pause_for_approval() {
        let result_awaiting = StepResult {
            id: "sr-1".to_string(),
            run_id: "run-1".to_string(),
            job_id: String::new(),
            name: "review".to_string(),
            status: StepStatus::AwaitingApproval,
            step_order: 0,
            exit_code: Some(0),
            duration_ms: Some(100),
            error: None,
            started_at: None,
            completed_at: None,
        };

        let result_passed = StepResult {
            status: StepStatus::Passed,
            ..result_awaiting.clone()
        };

        assert!(should_pause_for_approval(&result_awaiting));
        assert!(!should_pause_for_approval(&result_passed));
    }

    #[tokio::test]
    async fn test_execute_stage_with_require_approval() {
        let temp_dir = TempDir::new().unwrap();
        let env = create_test_env(&temp_dir);

        let mut stage = create_test_stage("review", "true");
        stage.require_approval = true;

        let result = execute_stage_with_log_callback(
            &stage,
            "sr-1",
            "run-1",
            &env,
            Duration::from_secs(10),
            None,
            None,
        )
        .await
        .unwrap();

        // Stage passed but requires approval
        assert_eq!(result.status, StepStatus::AwaitingApproval);
        assert_eq!(result.exit_code, Some(0));
    }

    #[tokio::test]
    async fn test_execute_stage_failed() {
        let temp_dir = TempDir::new().unwrap();
        let env = create_test_env(&temp_dir);

        let stage = create_test_stage("test", "exit 42");

        let result = execute_stage_with_log_callback(
            &stage,
            "sr-1",
            "run-1",
            &env,
            Duration::from_secs(10),
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(result.status, StepStatus::Failed);
        assert_eq!(result.exit_code, Some(42));
        assert!(result.error.is_some());
    }

    #[test]
    fn test_create_stage_logs_dir() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());

        // With job_key: logs/{job_key}/{stage_name}/
        let logs_dir =
            create_stage_logs_dir(&paths, "repo-1", "run-1", "test", Some("lint")).unwrap();
        assert!(logs_dir.exists());
        assert!(logs_dir.ends_with("logs/lint/test"));

        // Without job_key: logs/{stage_name}/
        let logs_dir2 = create_stage_logs_dir(&paths, "repo-1", "run-1", "test", None).unwrap();
        assert!(logs_dir2.exists());
        assert!(logs_dir2.ends_with("logs/test"));
    }

    #[test]
    fn test_create_run_artifacts_dir() {
        let temp_dir = TempDir::new().unwrap();
        let paths = AirlockPaths::with_root(temp_dir.path().to_path_buf());

        let run_artifacts_dir = create_run_artifacts_dir(&paths, "repo-1", "run-1").unwrap();

        assert!(run_artifacts_dir.exists());
        assert!(run_artifacts_dir.join("logs").exists());
    }

    #[tokio::test]
    async fn test_write_stage_logs() {
        let temp_dir = TempDir::new().unwrap();
        let logs_dir = temp_dir.path().join("logs");
        std::fs::create_dir_all(&logs_dir).unwrap();

        let result = StageExecutionResult {
            exit_code: 0,
            stdout: "stdout content".to_string(),
            stderr: "stderr content".to_string(),
            duration_ms: 100,
            passed: true,
        };

        write_stage_logs(&logs_dir, &result).unwrap();

        assert!(logs_dir.join("stdout.log").exists());
        assert!(logs_dir.join("stderr.log").exists());

        let stdout_content = std::fs::read_to_string(logs_dir.join("stdout.log")).unwrap();
        let stderr_content = std::fs::read_to_string(logs_dir.join("stderr.log")).unwrap();

        assert_eq!(stdout_content, "stdout content");
        assert_eq!(stderr_content, "stderr content");
    }

    #[tokio::test]
    async fn test_log_callback_writes_to_disk_during_streaming() {
        let temp_dir = TempDir::new().unwrap();
        let env = create_test_env(&temp_dir);

        // Build a disk-writing callback matching the pattern used by handlers
        let logs_dir = env.logs_dir.clone();
        let callback: LogStreamCallback = Arc::new(move |stream_type: &str, content: String| {
            let filename = if stream_type == "stdout" {
                "stdout.log"
            } else {
                "stderr.log"
            };
            let path = logs_dir.join(filename);
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .and_then(|mut f| {
                    use std::io::Write;
                    f.write_all(content.as_bytes())
                })
                .unwrap();
        });

        let stage = create_test_stage("test", "echo 'hello stdout' && echo 'hello stderr' >&2");
        let result = execute_stage_command_with_streaming(
            &stage,
            &env,
            Duration::from_secs(10),
            Some(callback),
            None,
        )
        .await
        .unwrap();

        assert!(result.passed);

        // Verify logs were written to disk by the callback during execution
        let stdout = std::fs::read_to_string(env.logs_dir.join("stdout.log")).unwrap();
        let stderr = std::fs::read_to_string(env.logs_dir.join("stderr.log")).unwrap();
        assert!(
            stdout.contains("hello stdout"),
            "stdout.log should contain output written during streaming, got: {stdout}"
        );
        assert!(
            stderr.contains("hello stderr"),
            "stderr.log should contain output written during streaming, got: {stderr}"
        );
    }

    /// Helper to create a git repo with commits for testing base SHA resolution.
    fn create_test_git_repo(temp_dir: &TempDir) -> PathBuf {
        let repo_path = temp_dir.path().join("repo");
        std::fs::create_dir_all(&repo_path).unwrap();

        // Initialize git repo
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Configure git user for commits
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        repo_path
    }

    /// Helper to create a commit and return its SHA.
    fn create_commit(repo_path: &Path, message: &str) -> String {
        // Create/modify a file
        let file_path = repo_path.join("file.txt");
        std::fs::write(&file_path, message).unwrap();

        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(repo_path)
            .output()
            .unwrap();

        std::process::Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(repo_path)
            .output()
            .unwrap();

        // Get the commit SHA
        let output = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo_path)
            .output()
            .unwrap();

        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn test_is_ancestor_true() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_git_repo(&temp_dir);

        // Create two commits: A -> B (B is HEAD)
        let commit_a = create_commit(&repo_path, "commit A");
        let _commit_b = create_commit(&repo_path, "commit B");

        // A should be an ancestor of HEAD (B)
        assert!(is_ancestor(&repo_path, &commit_a));
    }

    #[test]
    fn test_is_ancestor_false_diverged() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_git_repo(&temp_dir);

        // Create initial commit and rename branch to "main" for consistency
        let _commit_a = create_commit(&repo_path, "commit A");
        std::process::Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // Create a feature branch and add a commit
        std::process::Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        let _commit_b = create_commit(&repo_path, "commit B on feature");

        // Go back to main and create a different commit (simulating diverged history)
        std::process::Command::new("git")
            .args(["checkout", "main"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        let commit_c = create_commit(&repo_path, "commit C on main");

        // Go back to feature branch
        std::process::Command::new("git")
            .args(["checkout", "feature"])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        // commit_c (from main, created after feature branched) is NOT an ancestor of HEAD (feature branch)
        assert!(!is_ancestor(&repo_path, &commit_c));
    }

    #[test]
    fn test_is_ancestor_nonexistent_commit() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_git_repo(&temp_dir);
        create_commit(&repo_path, "commit A");

        // A fake SHA should not be an ancestor
        assert!(!is_ancestor(
            &repo_path,
            "0000000000000000000000000000000000000000"
        ));
        assert!(!is_ancestor(
            &repo_path,
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
        ));
    }

    #[test]
    fn test_resolve_effective_base_sha_normal_push() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_git_repo(&temp_dir);

        // Create two commits: A -> B
        let commit_a = create_commit(&repo_path, "commit A");
        let _commit_b = create_commit(&repo_path, "commit B");

        // Normal push: base_sha (A) is an ancestor of HEAD (B)
        let result = resolve_effective_base_sha(&repo_path, &commit_a).unwrap();
        assert_eq!(
            result, commit_a,
            "Normal push should use the provided base_sha"
        );
    }

    #[test]
    fn test_resolve_effective_base_sha_new_branch() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_git_repo(&temp_dir);

        // Create main branch with commits
        create_commit(&repo_path, "commit A");
        create_commit(&repo_path, "commit B");

        // Rename to main (in case default is master)
        let _ = std::process::Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(&repo_path)
            .output();

        // Create feature branch
        std::process::Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        create_commit(&repo_path, "commit C on feature");

        // New branch: base_sha is null
        let result = resolve_effective_base_sha(&repo_path, NULL_SHA).unwrap();

        // Should resolve to merge-base with main (which is commit B)
        assert_ne!(result, NULL_SHA, "Should not return null SHA");
        assert!(!result.is_empty(), "Should return a valid SHA");
    }

    #[test]
    fn test_resolve_effective_base_sha_force_push() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_git_repo(&temp_dir);

        // Create main branch
        create_commit(&repo_path, "main commit 1");
        let main_commit_2 = create_commit(&repo_path, "main commit 2");

        // Rename to main
        let _ = std::process::Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(&repo_path)
            .output();

        // Create feature branch from main
        std::process::Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        let old_feature_commit = create_commit(&repo_path, "old feature commit");

        // Simulate force push: reset feature branch to a new commit that doesn't include old_feature_commit
        std::process::Command::new("git")
            .args(["reset", "--hard", &main_commit_2])
            .current_dir(&repo_path)
            .output()
            .unwrap();
        create_commit(&repo_path, "new feature commit after force push");

        // Force push scenario: old_feature_commit is NOT an ancestor of the new HEAD
        let result = resolve_effective_base_sha(&repo_path, &old_feature_commit).unwrap();

        // Should NOT use old_feature_commit, should compute merge-base instead
        assert_ne!(
            result, old_feature_commit,
            "Force push should NOT use the old (overwritten) base_sha"
        );
        assert!(!result.is_empty(), "Should return a valid SHA");
    }

    #[test]
    fn test_append_log_capped_normal() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.log");

        append_log_capped(&log_path, b"hello ");
        append_log_capped(&log_path, b"world");

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_append_log_capped_truncates_at_limit() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("test.log");

        // Write a file that's just under the limit
        let filler = vec![b'x'; MAX_LOG_FILE_SIZE as usize - 10];
        std::fs::write(&log_path, &filler).unwrap();

        // This write would exceed the cap — should write truncation marker instead
        append_log_capped(&log_path, b"this is way too long to fit");

        let content = std::fs::read(&log_path).unwrap();
        let tail = String::from_utf8_lossy(&content[filler.len()..]);
        assert!(
            tail.contains("[log truncated at 50 MB]"),
            "Expected truncation marker, got: {tail}"
        );

        // Further writes should be silently dropped (file is now over the cap)
        let size_before = std::fs::metadata(&log_path).unwrap().len();
        append_log_capped(&log_path, b"should be dropped");
        let size_after = std::fs::metadata(&log_path).unwrap().len();
        assert_eq!(
            size_before, size_after,
            "No data should be appended after truncation"
        );
    }

    #[test]
    fn test_append_log_capped_new_file() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("new.log");

        assert!(!log_path.exists());
        append_log_capped(&log_path, b"created");
        assert!(log_path.exists());

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert_eq!(content, "created");
    }

    /// E2E: A git commit created through the stage executor uses the
    /// configured author and Airlock as committer.
    #[tokio::test]
    async fn test_git_commit_through_stage_uses_configured_identity() {
        let temp_dir = TempDir::new().unwrap();
        let mut env = create_test_env(&temp_dir);

        // Initialize a real git repo as the worktree
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&env.worktree)
            .output()
            .unwrap();

        // Create a file so we can commit it
        std::fs::write(env.worktree.join("test.txt"), "hello").unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&env.worktree)
            .output()
            .unwrap();

        // Set author identity on the stage environment
        env.git_author_name = Some("Alice Developer".to_string());
        env.git_author_email = Some("alice@dev.example".to_string());

        // Run a git commit through the stage executor
        let stage = create_test_stage("freeze", "git commit -m 'auto-fix commit'");
        let result =
            execute_stage_command_with_streaming(&stage, &env, Duration::from_secs(10), None, None)
                .await
                .unwrap();
        assert!(result.passed, "commit should succeed: {}", result.stderr);

        // Verify the author on the resulting commit
        let author_output = std::process::Command::new("git")
            .args(["log", "-1", "--format=%an <%ae>"])
            .current_dir(&env.worktree)
            .output()
            .unwrap();
        let author = String::from_utf8_lossy(&author_output.stdout)
            .trim()
            .to_string();
        assert_eq!(
            author, "Alice Developer <alice@dev.example>",
            "Commit author should match the working repo user config"
        );

        // Verify the committer on the resulting commit
        let committer_output = std::process::Command::new("git")
            .args(["log", "-1", "--format=%cn <%ce>"])
            .current_dir(&env.worktree)
            .output()
            .unwrap();
        let committer = String::from_utf8_lossy(&committer_output.stdout)
            .trim()
            .to_string();
        assert_eq!(
            committer, "Airlock <airlock@airlockhq.com>",
            "Commit committer should be Airlock"
        );
    }
}
