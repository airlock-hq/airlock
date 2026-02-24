//! Individual diagnostic check functions for the doctor command.

use super::DiagnosticResult;
use airlock_core::{git, AirlockPaths, Database};
use std::path::Path;
use tracing::debug;

/// Check if the daemon is running by verifying the socket exists.
pub(crate) fn check_daemon(paths: &AirlockPaths) -> DiagnosticResult {
    let socket_path = paths.socket();

    debug!("Checking daemon socket at: {}", socket_path.display());

    // On Unix, check if the socket file exists
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;

        if !socket_path.exists() {
            return DiagnosticResult::fail(
                "Daemon",
                "Daemon socket not found",
                "Run 'airlock daemon start' to start the daemon",
            );
        }

        // Try to connect to verify the daemon is actually listening
        match UnixStream::connect(&socket_path) {
            Ok(_) => DiagnosticResult::pass("Daemon", "Daemon is running"),
            Err(e) => {
                debug!("Failed to connect to daemon socket: {}", e);
                DiagnosticResult::fail(
                    "Daemon",
                    format!("Socket exists but daemon not responding: {}", e),
                    "Try 'airlock daemon restart' to restart the daemon",
                )
            }
        }
    }

    #[cfg(windows)]
    {
        // On Windows, check named pipe
        // For now, just check if the socket path marker exists
        if socket_path.exists() {
            DiagnosticResult::pass("Daemon", "Daemon marker found (Windows)")
        } else {
            DiagnosticResult::fail(
                "Daemon",
                "Daemon not detected",
                "Run 'airlock daemon start' to start the daemon",
            )
        }
    }
}

/// Check database integrity.
pub(crate) fn check_database(paths: &AirlockPaths) -> DiagnosticResult {
    let db_path = paths.database();

    debug!("Checking database at: {}", db_path.display());

    if !db_path.exists() {
        return DiagnosticResult::fail(
            "Database",
            "Database file not found",
            "Database will be created automatically when you run 'airlock init'",
        );
    }

    // Try to open and query the database
    match Database::open(&db_path) {
        Ok(db) => {
            // Verify we can query repos
            match db.list_repos() {
                Ok(repos) => DiagnosticResult::pass(
                    "Database",
                    format!("Database OK ({} repos enrolled)", repos.len()),
                ),
                Err(e) => DiagnosticResult::fail(
                    "Database",
                    format!("Database query failed: {}", e),
                    "Try deleting ~/.airlock/state.sqlite and re-initializing your repos",
                ),
            }
        }
        Err(e) => DiagnosticResult::fail(
            "Database",
            format!("Failed to open database: {}", e),
            "Try deleting ~/.airlock/state.sqlite and re-initializing your repos",
        ),
    }
}

/// Check if the current directory is enrolled in Airlock.
pub(crate) fn check_repo_enrollment(working_dir: &Path, paths: &AirlockPaths) -> DiagnosticResult {
    // First check if we're in a git repo at all
    let working_repo = match git::discover_repo(working_dir) {
        Ok(repo) => repo,
        Err(_) => {
            return DiagnosticResult::warn(
                "Repository",
                "Not inside a Git repository (skipping repo-specific checks)",
            );
        }
    };

    let working_path = match working_repo.workdir() {
        Some(path) => match path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                return DiagnosticResult::fail(
                    "Repository",
                    "Cannot resolve working directory path",
                    "Ensure the repository path is accessible",
                );
            }
        },
        None => {
            return DiagnosticResult::warn(
                "Repository",
                "Inside a bare repository (skipping repo-specific checks)",
            );
        }
    };

    // Check if repo is in database
    let db_path = paths.database();
    if !db_path.exists() {
        return DiagnosticResult::fail(
            "Repository",
            "Repository not enrolled (database not found)",
            "Run 'airlock init' to enroll this repository",
        );
    }

    match Database::open(&db_path) {
        Ok(db) => match db.get_repo_by_path(&working_path) {
            Ok(Some(_)) => {
                DiagnosticResult::pass("Repository", "Repository is enrolled in Airlock")
            }
            Ok(None) => DiagnosticResult::fail(
                "Repository",
                "Repository is not enrolled in Airlock",
                "Run 'airlock init' to enroll this repository",
            ),
            Err(e) => DiagnosticResult::fail(
                "Repository",
                format!("Failed to check enrollment: {}", e),
                "Try running 'airlock init' to re-enroll",
            ),
        },
        Err(e) => DiagnosticResult::fail(
            "Repository",
            format!("Cannot check enrollment: {}", e),
            "Database may be corrupted. Try deleting ~/.airlock/state.sqlite",
        ),
    }
}

/// Get the enrolled repo info for the current working directory.
pub(crate) fn get_enrolled_repo(
    working_dir: &Path,
    paths: &AirlockPaths,
) -> anyhow::Result<Option<airlock_core::Repo>> {
    let working_repo = git::discover_repo(working_dir)?;
    let working_path = working_repo
        .workdir()
        .ok_or_else(|| anyhow::anyhow!("Bare repository"))?
        .canonicalize()?;

    let db = Database::open(&paths.database())?;
    db.get_repo_by_path(&working_path)
        .map_err(|e| anyhow::anyhow!("{}", e))
}

/// Check if remotes are correctly configured.
pub(crate) fn check_remotes(working_dir: &Path, repo: &airlock_core::Repo) -> DiagnosticResult {
    let working_repo = match git::discover_repo(working_dir) {
        Ok(r) => r,
        Err(e) => {
            return DiagnosticResult::fail(
                "Remotes",
                format!("Cannot open repository: {}", e),
                "Ensure you're in a valid Git repository",
            );
        }
    };

    // Check origin remote points to gate
    let origin_url = match git::get_remote_url(&working_repo, "origin") {
        Ok(url) => url,
        Err(_) => {
            return DiagnosticResult::fail(
                "Remotes",
                "'origin' remote not found",
                "Run 'airlock init' to reconfigure remotes",
            );
        }
    };

    let gate_path_str = repo.gate_path.to_string_lossy();
    if !origin_url.contains(&*gate_path_str) && origin_url != gate_path_str.as_ref() {
        return DiagnosticResult::fail(
            "Remotes",
            format!(
                "'origin' points to '{}' but should point to gate at '{}'",
                origin_url,
                repo.gate_path.display()
            ),
            "Run 'airlock eject' then 'airlock init' to reconfigure",
        );
    }

    // Check upstream remote exists and points to the right URL
    let upstream_url = match git::get_remote_url(&working_repo, "upstream") {
        Ok(url) => url,
        Err(_) => {
            return DiagnosticResult::fail(
                "Remotes",
                "'upstream' remote not found",
                "Run 'airlock eject' then 'airlock init' to reconfigure",
            );
        }
    };

    if upstream_url != repo.upstream_url {
        return DiagnosticResult::fail(
            "Remotes",
            format!(
                "'upstream' points to '{}' but should point to '{}'",
                upstream_url, repo.upstream_url
            ),
            "Update upstream remote with: git remote set-url upstream <correct-url>",
        );
    }

    DiagnosticResult::pass(
        "Remotes",
        "Remote configuration is correct (origin → gate, upstream → remote)",
    )
}

/// Check if hooks are installed in the gate repo.
pub(crate) fn check_hooks(gate_path: &Path) -> DiagnosticResult {
    let hooks_dir = gate_path.join("hooks");

    if !hooks_dir.exists() {
        return DiagnosticResult::fail(
            "Hooks",
            "Hooks directory not found in gate repo",
            "Run 'airlock eject' then 'airlock init' to reinstall hooks",
        );
    }

    let required_hooks = ["pre-receive", "post-receive"];
    let mut missing_hooks = Vec::new();
    let mut non_executable_hooks = Vec::new();

    for hook_name in &required_hooks {
        let hook_path = hooks_dir.join(hook_name);

        if !hook_path.exists() {
            missing_hooks.push(*hook_name);
            continue;
        }

        // Check if executable (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = std::fs::metadata(&hook_path) {
                let perms = metadata.permissions();
                if perms.mode() & 0o111 == 0 {
                    non_executable_hooks.push(*hook_name);
                }
            }
        }
    }

    if !missing_hooks.is_empty() {
        return DiagnosticResult::fail(
            "Hooks",
            format!("Missing hooks: {}", missing_hooks.join(", ")),
            "Run 'airlock eject' then 'airlock init' to reinstall hooks",
        );
    }

    if !non_executable_hooks.is_empty() {
        return DiagnosticResult::fail(
            "Hooks",
            format!("Hooks not executable: {}", non_executable_hooks.join(", ")),
            format!(
                "Run: chmod +x {}",
                non_executable_hooks
                    .iter()
                    .map(|h| format!("{}/hooks/{}", gate_path.display(), h))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        );
    }

    DiagnosticResult::pass("Hooks", "All hooks installed and executable")
}

/// Check if the gate repo exists and is valid.
pub(crate) fn check_gate_repo(gate_path: &Path) -> DiagnosticResult {
    if !gate_path.exists() {
        return DiagnosticResult::fail(
            "Gate Repo",
            format!("Gate repository not found at {}", gate_path.display()),
            "Run 'airlock eject' then 'airlock init' to recreate the gate",
        );
    }

    // Try to open as a git repo
    match git::open_repo(gate_path) {
        Ok(repo) => {
            if !repo.is_bare() {
                return DiagnosticResult::fail(
                    "Gate Repo",
                    "Gate repository is not bare",
                    "Run 'airlock eject' then 'airlock init' to recreate the gate",
                );
            }

            // Check origin remote in gate
            match git::get_remote_url(&repo, "origin") {
                Ok(_) => DiagnosticResult::pass(
                    "Gate Repo",
                    format!("Gate repository OK at {}", gate_path.display()),
                ),
                Err(_) => DiagnosticResult::fail(
                    "Gate Repo",
                    "Gate repository missing 'origin' remote",
                    "Run 'airlock eject' then 'airlock init' to reconfigure",
                ),
            }
        }
        Err(e) => DiagnosticResult::fail(
            "Gate Repo",
            format!("Cannot open gate repository: {}", e),
            "Run 'airlock eject' then 'airlock init' to recreate the gate",
        ),
    }
}
