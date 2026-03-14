//! Database export functionality for generating fixtures from real data.
//!
//! This module reads data from the Airlock SQLite database and exports it
//! using the exact same IPC types from `airlock_daemon::ipc`, ensuring that
//! generated fixtures match production serialization exactly.

use anyhow::{Context, Result};
use std::path::Path;

// Import database and types from airlock-core
use airlock_core::{step_status_to_string, AirlockPaths, Database, Run};

// Import IPC types from airlock-daemon for exact serialization matching
use airlock_daemon::ipc::{
    ArtifactInfo, GetReposResult, GetRunDetailResult, GetRunsResult, RefUpdateParam,
    RepoWithStatus, RunDetailInfo, RunInfo, StepResultInfo,
};
use serde::Serialize;

/// Result of reading artifact content (matches frontend type)
#[derive(Debug, Serialize)]
pub struct ReadArtifactResult {
    pub content: String,
    pub is_binary: bool,
    pub total_size: u64,
    pub bytes_read: u64,
    pub offset: u64,
}

/// Result of getting run diff (matches frontend type)
#[derive(Debug, Serialize)]
pub struct GetRunDiffResult {
    pub run_id: String,
    pub branch: String,
    pub base_sha: String,
    pub head_sha: String,
    pub patch: String,
    pub files_changed: Vec<String>,
    pub additions: u32,
    pub deletions: u32,
}

/// Export all fixtures from the database to the output directory.
///
/// This creates the following structure:
/// ```
/// output_dir/
/// ├── get_repos/
/// │   └── all.json
/// ├── get_runs/
/// │   └── <repo_id>.json (one per repo)
/// └── get_run_detail/
///     └── <run_id>.json (one per run)
/// ```
pub fn export_from_database(db_path: &Path, output_dir: &Path) -> Result<()> {
    println!("Opening database at: {}", db_path.display());

    let db = Database::open(db_path).context("Failed to open database")?;

    // Create AirlockPaths for artifact loading
    let paths = AirlockPaths::new().context("Failed to get Airlock paths")?;

    // Export repos
    let exported_repos = export_repos(&db, output_dir)?;
    println!("  Exported {} repos", exported_repos.len());

    // Export runs for each repo
    let mut total_runs = 0;
    for repo in &exported_repos {
        let exported_runs = export_runs(&db, &repo.id, output_dir)?;
        total_runs += exported_runs.len();

        // Export run details for each run
        for run in &exported_runs {
            export_run_detail(&db, &paths, &run.id, output_dir)?;
        }
    }
    println!("  Exported {} runs", total_runs);

    // Export artifact content for all runs
    let mut total_artifacts = 0;
    for repo in &exported_repos {
        let runs = db.list_runs(&repo.id, None).unwrap_or_default();
        for run in &runs {
            total_artifacts += export_artifact_content(&paths, &repo.id, &run.id, output_dir)?;
        }
    }
    if total_artifacts > 0 {
        println!("  Exported {} artifact contents", total_artifacts);
    }

    // Export run diffs
    let mut total_diffs = 0;
    for repo in &exported_repos {
        let db_repo = db.get_repo(&repo.id)?;
        if let Some(db_repo) = db_repo {
            let runs = db.list_runs(&repo.id, None).unwrap_or_default();
            for run in &runs {
                if export_run_diff(&db_repo.gate_path, run, output_dir)? {
                    total_diffs += 1;
                }
            }
        }
    }
    if total_diffs > 0 {
        println!("  Exported {} run diffs", total_diffs);
    }

    // Write index.json
    write_index(output_dir, &exported_repos)?;

    Ok(())
}

/// Simple repo info for tracking which repos were exported.
pub struct ExportedRepo {
    pub id: String,
}

/// Export all repositories to get_repos/all.json
fn export_repos(db: &Database, output_dir: &Path) -> Result<Vec<ExportedRepo>> {
    let repos = db.list_repos().context("Failed to list repos")?;

    let mut repos_with_status = Vec::with_capacity(repos.len());
    let mut exported = Vec::with_capacity(repos.len());

    for repo in repos {
        // Count pending runs using derived status
        let pending_runs = match db.list_runs(&repo.id, Some(100)) {
            Ok(runs) => {
                let mut count = 0u32;
                for r in &runs {
                    let stages = db.get_step_results_for_run(&r.id).unwrap_or_default();
                    let status = r.derived_status(&stages);
                    if status == "running" || status == "awaiting_approval" || status == "pending" {
                        count += 1;
                    }
                }
                count
            }
            Err(_) => 0,
        };

        // Check if gate path exists and is a valid git repo
        let gate_healthy = repo.gate_path.exists() && repo.gate_path.join("HEAD").exists();

        exported.push(ExportedRepo {
            id: repo.id.clone(),
        });

        repos_with_status.push(RepoWithStatus {
            id: repo.id,
            working_path: repo.working_path.to_string_lossy().to_string(),
            upstream_url: repo.upstream_url,
            gate_path: repo.gate_path.to_string_lossy().to_string(),
            created_at: repo.created_at,
            pending_runs,
            last_sync: repo.last_sync,
            gate_healthy,
        });
    }

    // Write to file
    let repos_dir = output_dir.join("get_repos");
    std::fs::create_dir_all(&repos_dir)?;

    let result = GetReposResult {
        repos: repos_with_status,
    };
    let json = serde_json::to_string_pretty(&result)?;
    let path = repos_dir.join("all.json");
    std::fs::write(&path, json)?;
    println!("  get_repos/all.json -> {}", path.display());

    Ok(exported)
}

/// Simple run info for tracking which runs were exported.
pub struct ExportedRun {
    pub id: String,
}

/// Export runs for a repository to get_runs/<repo_id>.json
fn export_runs(db: &Database, repo_id: &str, output_dir: &Path) -> Result<Vec<ExportedRun>> {
    let runs = db.list_runs(repo_id, None).context("Failed to list runs")?;

    let mut run_infos = Vec::with_capacity(runs.len());
    let mut exported = Vec::with_capacity(runs.len());

    for run in runs {
        let stages = db.get_step_results_for_run(&run.id).unwrap_or_default();
        let status = run.derived_status(&stages).to_string();
        let completed_at = if status == "completed" || status == "failed" {
            stages.iter().filter_map(|s| s.completed_at).max()
        } else {
            None
        };
        exported.push(ExportedRun { id: run.id.clone() });
        run_infos.push(RunInfo {
            id: run.id,
            repo_id: Some(repo_id.to_string()),
            status,
            branch: if run.branch.is_empty() {
                None
            } else {
                Some(run.branch)
            },
            base_sha: None,
            head_sha: None,
            current_step: run.current_step,
            created_at: run.created_at,
            updated_at: Some(run.updated_at),
            completed_at,
            error: run.error,
        });
    }

    // Write to file
    let runs_dir = output_dir.join("get_runs");
    std::fs::create_dir_all(&runs_dir)?;

    let result = GetRunsResult { runs: run_infos };
    let json = serde_json::to_string_pretty(&result)?;
    let path = runs_dir.join(format!("{}.json", repo_id));
    std::fs::write(&path, json)?;
    println!("  get_runs/{}.json -> {}", repo_id, path.display());

    Ok(exported)
}

/// Export run detail to get_run_detail/<run_id>.json
fn export_run_detail(
    db: &Database,
    paths: &AirlockPaths,
    run_id: &str,
    output_dir: &Path,
) -> Result<()> {
    let run = db
        .get_run(run_id)
        .context("Failed to get run")?
        .ok_or_else(|| anyhow::anyhow!("Run not found: {}", run_id))?;

    // Get stage results
    let db_stage_results = db.get_step_results_for_run(run_id).unwrap_or_default();

    // Compute derived status
    let status = run.derived_status(&db_stage_results).to_string();
    let completed_at = if status == "completed" || status == "failed" {
        db_stage_results.iter().filter_map(|s| s.completed_at).max()
    } else {
        None
    };

    // Convert stage results to IPC format
    let step_results: Vec<StepResultInfo> = db_stage_results
        .iter()
        .map(|sr| StepResultInfo {
            id: sr.id.clone(),
            job_id: sr.job_id.clone(),
            job_key: String::new(),
            step: sr.name.clone(),
            status: step_status_to_string(sr.status).to_string(),
            exit_code: sr.exit_code,
            duration_ms: sr.duration_ms.map(|d| d as u64),
            error: sr.error.clone(),
            started_at: sr.started_at,
            completed_at: sr.completed_at,
        })
        .collect();

    // Load artifacts from filesystem
    let artifacts = load_artifacts(paths, &run.repo_id, run_id);

    let result = GetRunDetailResult {
        run: RunDetailInfo {
            id: run.id.clone(),
            repo_id: run.repo_id.clone(),
            status,
            branch: run.branch.clone(),
            base_sha: run.base_sha.clone(),
            head_sha: run.head_sha.clone(),
            current_step: run.current_step.clone(),
            workflow_file: run.workflow_file.clone(),
            workflow_name: run.workflow_name.clone(),
            ref_updates: run
                .ref_updates
                .into_iter()
                .map(|r| RefUpdateParam {
                    ref_name: r.ref_name,
                    old_sha: r.old_sha,
                    new_sha: r.new_sha,
                })
                .collect(),
            error: run.error,
            created_at: run.created_at,
            updated_at: run.updated_at,
            completed_at,
        },
        jobs: vec![],
        step_results,
        artifacts,
    };

    // Write to file
    let detail_dir = output_dir.join("get_run_detail");
    std::fs::create_dir_all(&detail_dir)?;

    let json = serde_json::to_string_pretty(&result)?;
    let path = detail_dir.join(format!("{}.json", run_id));
    std::fs::write(&path, json)?;
    println!("  get_run_detail/{}.json -> {}", run_id, path.display());

    Ok(())
}

/// Export artifact content for a run to read_artifact/<hash>.json
fn export_artifact_content(
    paths: &AirlockPaths,
    repo_id: &str,
    run_id: &str,
    output_dir: &Path,
) -> Result<usize> {
    let artifact_dir = paths.run_artifacts(repo_id, run_id);
    let mut count = 0;

    if !artifact_dir.exists() {
        return Ok(0);
    }

    let read_artifact_dir = output_dir.join("read_artifact");
    std::fs::create_dir_all(&read_artifact_dir)?;

    // Process content/ directory
    let content_dir = artifact_dir.join("content");
    if content_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&content_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }

                // Read file content
                let content = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue, // Skip binary files
                };

                let total_size = content.len();

                // Create ReadArtifactResult
                let result = ReadArtifactResult {
                    content,
                    is_binary: false,
                    total_size: total_size as u64,
                    bytes_read: total_size as u64,
                    offset: 0,
                };

                // Use the artifact path as the key (URL-safe hash)
                let artifact_path = path.to_string_lossy().to_string();
                let hash = simple_hash(&artifact_path);
                let json = serde_json::to_string_pretty(&result)?;
                let fixture_path = read_artifact_dir.join(format!("{}.json", hash));
                std::fs::write(&fixture_path, json)?;

                // Also write a mapping file so we can look up by path
                let mapping_path = read_artifact_dir.join(format!("{}.path", hash));
                std::fs::write(&mapping_path, &artifact_path)?;

                count += 1;
            }
        }
    }

    Ok(count)
}

/// Simple hash function for creating fixture filenames from paths
fn simple_hash(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Export run diff to get_run_diff/<run_id>.json
fn export_run_diff(gate_path: &std::path::Path, run: &Run, output_dir: &Path) -> Result<bool> {
    // base_sha and head_sha are String, not Option<String>
    let base_sha = &run.base_sha;
    if base_sha.is_empty() || base_sha == "0000000000000000000000000000000000000000" {
        return Ok(false); // No valid base sha
    }

    let head_sha = &run.head_sha;
    if head_sha.is_empty() {
        return Ok(false); // No valid head sha
    }

    // Run git diff in the gate repo
    let output = std::process::Command::new("git")
        .args(["diff", base_sha, head_sha])
        .current_dir(gate_path)
        .output();

    let patch = match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
        _ => return Ok(false), // Git diff failed
    };

    // Count additions and deletions
    let mut additions = 0u32;
    let mut deletions = 0u32;
    let mut files_changed = Vec::new();

    for line in patch.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            additions += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            deletions += 1;
        } else if line.starts_with("diff --git") {
            // Extract filename from "diff --git a/path b/path"
            if let Some(path) = line.split(" b/").nth(1) {
                files_changed.push(path.to_string());
            }
        }
    }

    let result = GetRunDiffResult {
        run_id: run.id.clone(),
        branch: run.branch.clone(),
        base_sha: base_sha.clone(),
        head_sha: head_sha.clone(),
        patch,
        files_changed,
        additions,
        deletions,
    };

    // Write to file
    let diff_dir = output_dir.join("get_run_diff");
    std::fs::create_dir_all(&diff_dir)?;

    let json = serde_json::to_string_pretty(&result)?;
    let path = diff_dir.join(format!("{}.json", run.id));
    std::fs::write(&path, json)?;

    Ok(true)
}

/// Write index.json listing all exported fixtures
fn write_index(output_dir: &Path, repos: &[ExportedRepo]) -> Result<()> {
    let mut index = serde_json::Map::new();

    // List get_repos fixtures
    index.insert("get_repos".to_string(), serde_json::json!(["all"]));

    // List get_runs fixtures (one per repo)
    let runs_list: Vec<String> = repos.iter().map(|r| r.id.clone()).collect();
    index.insert("get_runs".to_string(), serde_json::json!(runs_list));

    // List get_run_detail fixtures (scan directory)
    let detail_dir = output_dir.join("get_run_detail");
    let mut details_list = Vec::new();
    if detail_dir.exists() {
        for entry in std::fs::read_dir(&detail_dir)? {
            let entry = entry?;
            if let Some(name) = entry.path().file_stem() {
                details_list.push(name.to_string_lossy().to_string());
            }
        }
    }
    index.insert(
        "get_run_detail".to_string(),
        serde_json::json!(details_list),
    );

    let json = serde_json::to_string_pretty(&serde_json::Value::Object(index))?;
    let path = output_dir.join("index.json");
    std::fs::write(&path, json)?;
    println!("  index.json -> {}", path.display());

    Ok(())
}

/// Load artifacts from a run's artifact directory.
///
/// This function is a simplified version of the one in airlock-daemon handlers.
fn load_artifacts(paths: &AirlockPaths, repo_id: &str, run_id: &str) -> Vec<ArtifactInfo> {
    let artifact_dir = paths.run_artifacts(repo_id, run_id);
    let mut artifacts = Vec::new();

    if !artifact_dir.exists() {
        return artifacts;
    }

    // Scan top-level entries
    if let Ok(entries) = std::fs::read_dir(&artifact_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            if path.is_dir() {
                // Scan artifact subdirectories: content/, comments/, patches/
                // Skip logs/ directory
                match name.as_str() {
                    "content" | "comments" | "patches" => {
                        artifacts.extend(load_subdirectory_artifacts(&path));
                    }
                    _ => {} // Skip logs/ and any other directories
                }
                continue;
            }

            // Skip log files
            if name.ends_with(".log") {
                continue;
            }

            // Determine artifact type from filename
            let artifact_type = determine_artifact_type(&name);

            // Get file metadata
            let metadata = std::fs::metadata(&path);
            let size_bytes = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            let created_at = metadata
                .as_ref()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            artifacts.push(ArtifactInfo {
                name,
                path: path.to_string_lossy().to_string(),
                artifact_type,
                size_bytes,
                created_at,
            });
        }
    }

    // Sort by name for consistent ordering
    artifacts.sort_by(|a, b| a.name.cmp(&b.name));

    artifacts
}

/// Load artifacts from a subdirectory (content/, comments/, patches/).
fn load_subdirectory_artifacts(dir: &std::path::Path) -> Vec<ArtifactInfo> {
    let mut artifacts = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let metadata = std::fs::metadata(&path);
            let size_bytes = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            let created_at = metadata
                .as_ref()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            artifacts.push(ArtifactInfo {
                name,
                path: path.to_string_lossy().to_string(),
                artifact_type: "file".to_string(),
                size_bytes,
                created_at,
            });
        }
    }

    artifacts
}

/// Determine artifact type from filename.
fn determine_artifact_type(filename: &str) -> String {
    match filename {
        "description.json" | "description.md" => "description".to_string(),
        "test_result.json" => "test_results".to_string(),
        "risk_assessment.json" => "risk_assessment".to_string(),
        "push_result.json" => "push".to_string(),
        "pr_result.json" => "pr".to_string(),
        "tour.json" => "tour".to_string(),
        "diff_analysis.json" => "analysis".to_string(),
        _ => std::path::Path::new(filename)
            .extension()
            .map(|ext| ext.to_string_lossy().to_lowercase())
            .unwrap_or_else(|| "unknown".to_string()),
    }
}
