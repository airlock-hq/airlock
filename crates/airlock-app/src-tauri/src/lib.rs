//! Airlock Desktop Application
//!
//! This is the Tauri backend for the Airlock desktop app. It provides commands
//! for communicating with the airlockd daemon via IPC.

mod event_bridge;
mod ipc;

use ipc::IpcClient;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, State, WindowEvent};

// Re-export shared IPC types from airlock-core for use throughout the app
pub use airlock_core::ipc::{
    ApplyPatchesResult, ApproveIntentResult, ApproveStepResult, ArtifactInfo, CommitDiffInfo,
    DiffHunkInfo, GetRunDiffResult, IntentDiffResult, IntentTourResult, JobResultInfo,
    LineAnnotationInfo, PatchError, RejectIntentResult, RunInfo, StepResultInfo, TourInfo,
    TourStepInfo,
};

/// Application state containing the IPC client
pub struct AppState {
    ipc_client: Arc<IpcClient>,
}

// =============================================================================
// Frontend Types (app-specific types not shared with daemon)
// =============================================================================

/// Repository information for the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoInfo {
    pub id: String,
    pub working_path: String,
    pub upstream_url: String,
    pub gate_path: String,
    pub created_at: i64,
    pub last_sync: Option<i64>,
    pub pending_runs: u32,
}

/// Run detail (app-specific wrapper around shared types)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunDetail {
    pub run: RunInfo,
    pub jobs: Vec<JobResultInfo>,
    pub step_results: Vec<StepResultInfo>,
    pub artifacts: Vec<ArtifactInfo>,
}

/// Status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub repo: RepoInfo,
    pub pending_runs: u32,
    pub latest_run: Option<RunInfo>,
}

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub healthy: bool,
    pub version: String,
    pub repo_count: u32,
    pub database_ok: bool,
}

// =============================================================================
// Tauri Commands
// =============================================================================

/// Check daemon health
#[tauri::command]
async fn check_health(state: State<'_, AppState>) -> Result<HealthResponse, String> {
    state.ipc_client.health().await.map_err(|e| e.to_string())
}

/// List all enrolled repositories
#[tauri::command]
async fn list_repos(state: State<'_, AppState>) -> Result<Vec<RepoInfo>, String> {
    state
        .ipc_client
        .list_repos()
        .await
        .map_err(|e| e.to_string())
}

/// Get status for a specific repository
#[tauri::command]
async fn get_repo_status(
    state: State<'_, AppState>,
    repo_id: String,
) -> Result<StatusResponse, String> {
    state
        .ipc_client
        .get_status(&repo_id)
        .await
        .map_err(|e| e.to_string())
}

/// Get runs for a repository
#[tauri::command]
async fn get_runs(
    state: State<'_, AppState>,
    repo_id: String,
    limit: Option<u32>,
) -> Result<Vec<RunInfo>, String> {
    state
        .ipc_client
        .get_runs(&repo_id, limit)
        .await
        .map_err(|e| e.to_string())
}

/// Get run detail
#[tauri::command]
async fn get_run_detail(state: State<'_, AppState>, run_id: String) -> Result<RunDetail, String> {
    state
        .ipc_client
        .get_run_detail(&run_id)
        .await
        .map_err(|e| e.to_string())
}

/// Get diff hunks for an intent
#[tauri::command]
async fn get_intent_diff(
    state: State<'_, AppState>,
    intent_id: String,
) -> Result<IntentDiffResult, String> {
    state
        .ipc_client
        .get_intent_diff(&intent_id)
        .await
        .map_err(|e| e.to_string())
}

/// Get guided tour for an intent
#[tauri::command]
async fn get_intent_tour(
    state: State<'_, AppState>,
    intent_id: String,
) -> Result<IntentTourResult, String> {
    state
        .ipc_client
        .get_intent_tour(&intent_id)
        .await
        .map_err(|e| e.to_string())
}

/// Sync a repository with upstream
#[tauri::command]
async fn sync_repo(state: State<'_, AppState>, repo_id: String) -> Result<bool, String> {
    state
        .ipc_client
        .sync_repo(&repo_id)
        .await
        .map_err(|e| e.to_string())
}

/// Sync all repositories with upstream
#[tauri::command]
async fn sync_all(state: State<'_, AppState>) -> Result<(u32, u32), String> {
    state.ipc_client.sync_all().await.map_err(|e| e.to_string())
}

/// Update the description of an intent
#[tauri::command]
async fn update_intent_description(
    state: State<'_, AppState>,
    intent_id: String,
    description: String,
) -> Result<String, String> {
    state
        .ipc_client
        .update_intent_description(&intent_id, &description)
        .await
        .map_err(|e| e.to_string())
}

/// Reprocess a run (re-run the full pipeline)
#[tauri::command]
async fn reprocess_run(state: State<'_, AppState>, run_id: String) -> Result<bool, String> {
    state
        .ipc_client
        .reprocess_run(&run_id)
        .await
        .map_err(|e| e.to_string())
}

/// Approve an intent (mark as ready for forwarding)
#[tauri::command]
async fn approve_intent(
    state: State<'_, AppState>,
    intent_id: String,
) -> Result<ApproveIntentResult, String> {
    state
        .ipc_client
        .approve_intent(&intent_id)
        .await
        .map_err(|e| e.to_string())
}

/// Reject an intent with an optional reason
#[tauri::command]
async fn reject_intent(
    state: State<'_, AppState>,
    intent_id: String,
    reason: Option<String>,
) -> Result<RejectIntentResult, String> {
    state
        .ipc_client
        .reject_intent(&intent_id, reason.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Approve a step (resume pipeline execution)
#[tauri::command]
async fn approve_step(
    state: State<'_, AppState>,
    run_id: String,
    job_key: String,
    step_name: String,
) -> Result<ApproveStepResult, String> {
    state
        .ipc_client
        .approve_step(&run_id, &job_key, &step_name)
        .await
        .map_err(|e| e.to_string())
}

/// Get the diff for a run
#[tauri::command]
async fn get_run_diff(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<GetRunDiffResult, String> {
    state
        .ipc_client
        .get_run_diff(&run_id)
        .await
        .map_err(|e| e.to_string())
}

/// Apply selected patches to a run
#[tauri::command]
async fn apply_patches(
    state: State<'_, AppState>,
    run_id: String,
    patch_paths: Vec<String>,
) -> Result<ApplyPatchesResult, String> {
    state
        .ipc_client
        .apply_patches(&run_id, &patch_paths)
        .await
        .map_err(|e| e.to_string())
}

/// Get configuration (global and optionally repo-specific)
#[tauri::command]
async fn get_config(
    state: State<'_, AppState>,
    repo_id: Option<String>,
) -> Result<airlock_core::ipc::GetConfigResult, String> {
    state
        .ipc_client
        .get_config(repo_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Update configuration (global and/or repo-specific)
#[tauri::command]
async fn update_config(
    state: State<'_, AppState>,
    global: Option<airlock_core::ipc::GlobalConfigUpdate>,
    repo: Option<airlock_core::ipc::RepoConfigUpdate>,
) -> Result<airlock_core::ipc::UpdateConfigResult, String> {
    state
        .ipc_client
        .update_config(global, repo)
        .await
        .map_err(|e| e.to_string())
}

/// Result of reading an artifact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadArtifactResult {
    pub content: String,
    pub is_binary: bool,
    /// Total size of the file in bytes
    pub total_size: u64,
    /// Number of bytes read in this response
    pub bytes_read: u64,
    /// Offset from which content was read
    pub offset: u64,
}

/// Read artifact content from a file path
/// Supports partial reads via offset and limit parameters for streaming large logs
#[tauri::command]
async fn read_artifact(
    artifact_path: String,
    offset: Option<u64>,
    limit: Option<u64>,
) -> Result<ReadArtifactResult, String> {
    use std::io::{Read, Seek, SeekFrom};
    use std::path::Path;

    let path = Path::new(&artifact_path);

    // Security check: only allow reading from airlock artifacts directory
    let airlock_paths = airlock_core::AirlockPaths::default();
    let artifacts_dir = airlock_paths.artifacts_dir();
    if !path.starts_with(&artifacts_dir) {
        return Err(format!(
            "Access denied: artifact path must be within {}",
            artifacts_dir.display()
        ));
    }

    if !path.exists() {
        return Err(format!("Artifact not found: {}", artifact_path));
    }

    // Get file metadata for total size
    let metadata =
        std::fs::metadata(path).map_err(|e| format!("Failed to read artifact metadata: {}", e))?;
    let total_size = metadata.len();

    // Open file for reading
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("Failed to open artifact: {}", e))?;

    // Seek to offset if specified
    let read_offset = offset.unwrap_or(0);
    if read_offset > 0 {
        file.seek(SeekFrom::Start(read_offset))
            .map_err(|e| format!("Failed to seek in artifact: {}", e))?;
    }

    // Calculate how many bytes to read
    let remaining = total_size.saturating_sub(read_offset);
    let bytes_to_read = match limit {
        Some(lim) => std::cmp::min(lim, remaining),
        None => remaining,
    };

    // Read the content
    let mut buffer = vec![0u8; bytes_to_read as usize];
    let bytes_actually_read = file
        .read(&mut buffer)
        .map_err(|e| format!("Failed to read artifact: {}", e))?;
    buffer.truncate(bytes_actually_read);

    // Check if content is valid UTF-8 (text) or binary
    match String::from_utf8(buffer.clone()) {
        Ok(content) => Ok(ReadArtifactResult {
            content,
            is_binary: false,
            total_size,
            bytes_read: bytes_actually_read as u64,
            offset: read_offset,
        }),
        Err(_) => {
            // Binary file - return base64 encoded or a placeholder message
            Ok(ReadArtifactResult {
                content: format!(
                    "[Binary file, {} bytes. Use the download button to save.]",
                    total_size
                ),
                is_binary: true,
                total_size,
                bytes_read: bytes_actually_read as u64,
                offset: read_offset,
            })
        }
    }
}

/// Show the main window. Called by the frontend once it has mounted,
/// ensuring the window-state plugin has already restored geometry.
#[tauri::command]
async fn show_window(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

// =============================================================================
// Application Entry Point
// =============================================================================

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let ipc_client = IpcClient::new();
    let app_state = AppState {
        ipc_client: Arc::new(ipc_client),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all()
                        & !tauri_plugin_window_state::StateFlags::VISIBLE,
                )
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri::plugin::Builder::<tauri::Wry>::new("external-links")
                .on_navigation(|_webview, url| {
                    if url.scheme() == "http" || url.scheme() == "https" {
                        // Allow localhost (Vite dev server) and Tauri's own URLs
                        if let Some(host) = url.host_str() {
                            if host == "localhost" || host == "127.0.0.1" {
                                return true;
                            }
                        }
                        let _ = tauri_plugin_opener::open_url(url.as_str(), None::<&str>);
                        return false;
                    }
                    true
                })
                .build(),
        )
        .manage(app_state)
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // Build tray icon menu
            let show_item = MenuItemBuilder::with_id("show", "Show").build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let tray_menu = MenuBuilder::new(app)
                .item(&show_item)
                .separator()
                .item(&quit_item)
                .build()?;

            // Build system tray icon
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Airlock")
                .menu(&tray_menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { .. } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Spawn the event bridge task to stream daemon events to the frontend
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                event_bridge::run_event_bridge(app_handle).await;
            });

            // Window stays hidden until the frontend calls show_window.
            // By that point the window-state plugin has already restored
            // size/position/maximized, so the window appears in its final
            // state with no resize flicker.

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Save window state before hiding (plugin normally saves on close,
                // but we prevent close for system tray behavior)
                use tauri_plugin_window_state::AppHandleExt;
                let _ = window
                    .app_handle()
                    .save_window_state(tauri_plugin_window_state::StateFlags::all());
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            check_health,
            list_repos,
            get_repo_status,
            get_runs,
            get_run_detail,
            get_intent_diff,
            get_intent_tour,
            reprocess_run,
            approve_intent,
            reject_intent,
            approve_step,
            get_run_diff,
            sync_repo,
            sync_all,
            update_intent_description,
            get_config,
            update_config,
            read_artifact,
            apply_patches,
            show_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
