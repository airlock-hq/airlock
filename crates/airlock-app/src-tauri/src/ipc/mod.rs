//! IPC client for communicating with the Airlock daemon.
//!
//! This module provides a client that communicates with airlockd using
//! JSON-RPC 2.0 over Unix domain sockets (or named pipes on Windows).

mod config;
mod error;
mod intents;
mod operations;
mod repos;
mod runs;
mod types;

use airlock_core::AirlockPaths;
use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::tokio::Stream;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;

// Re-export public types
pub use error::IpcError;

use types::{Request, Response};

/// IPC client for communicating with the Airlock daemon
pub struct IpcClient {
    paths: AirlockPaths,
    request_id: AtomicU32,
}

impl IpcClient {
    /// Create a new IPC client
    pub fn new() -> Self {
        Self {
            paths: AirlockPaths::default(),
            request_id: AtomicU32::new(1),
        }
    }

    /// Get the next request ID
    fn next_id(&self) -> u32 {
        self.request_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Connect to the daemon
    #[cfg(unix)]
    async fn connect(&self) -> Result<Stream, IpcError> {
        let socket_name = self.paths.socket_name();
        let name = socket_name
            .to_fs_name::<GenericFilePath>()
            .map_err(|e| IpcError::ConnectionError(e.to_string()))?;

        Stream::connect(name)
            .await
            .map_err(|e| IpcError::ConnectionError(e.to_string()))
    }

    /// Connect to the daemon (Windows)
    #[cfg(windows)]
    async fn connect(&self) -> Result<Stream, IpcError> {
        let socket_name = self.paths.socket_name();
        let name = socket_name
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| IpcError::ConnectionError(e.to_string()))?;

        Stream::connect(name)
            .await
            .map_err(|e| IpcError::ConnectionError(e.to_string()))
    }

    /// Send a request and get a response
    pub(crate) async fn send_request(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, IpcError> {
        let stream = self.connect().await?;
        let (reader, mut writer) = stream.split();
        let mut reader = BufReader::new(reader);

        let request = Request {
            jsonrpc: "2.0",
            method,
            params,
            id: self.next_id(),
        };

        // Send request
        let request_json = serde_json::to_string(&request)?;
        writer
            .write_all(request_json.as_bytes())
            .await
            .map_err(|e| IpcError::SendError(e.to_string()))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|e| IpcError::SendError(e.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|e| IpcError::SendError(e.to_string()))?;

        // Read response
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| IpcError::ReceiveError(e.to_string()))?;

        let response: Response = serde_json::from_str(&line)?;

        if let Some(error) = response.error {
            return Err(IpcError::RpcError {
                code: error.code,
                message: error.message,
            });
        }

        response.result.ok_or_else(|| IpcError::RpcError {
            code: -32603,
            message: "No result in response".to_string(),
        })
    }
}

impl Default for IpcClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use airlock_core::ipc::RunInfo;
    use types::*;

    #[test]
    fn test_ipc_client_creation() {
        let client = IpcClient::new();
        assert_eq!(client.next_id(), 1);
        assert_eq!(client.next_id(), 2);
    }

    /// Test that DaemonGetRunsResult correctly parses run status for completed pipelines.
    #[test]
    fn test_parse_run_status_for_pipeline_completion() {
        let completed_statuses = vec!["pendingapproval", "approved", "forwarded", "failed"];

        for status in &completed_statuses {
            let json = serde_json::json!({
                "runs": [
                    {
                        "id": "run-123",
                        "status": status,
                        "created_at": 1704067200,
                        "completed_at": 1704067260
                    }
                ]
            });

            let result: DaemonGetRunsResult = serde_json::from_value(json).unwrap();
            assert_eq!(result.runs.len(), 1);
            assert_eq!(result.runs[0].status, *status);
            assert!(
                result.runs[0].completed_at.is_some(),
                "Completed runs should have completed_at"
            );
        }

        // Test parsing a run that is still in progress
        let in_progress_json = serde_json::json!({
            "runs": [
                {
                    "id": "run-456",
                    "status": "running",
                    "created_at": 1704067200,
                    "completed_at": null
                }
            ]
        });

        let in_progress_result: DaemonGetRunsResult =
            serde_json::from_value(in_progress_json).unwrap();
        assert_eq!(in_progress_result.runs[0].status, "running");
        assert!(
            in_progress_result.runs[0].completed_at.is_none(),
            "Running runs should not have completed_at"
        );
    }

    /// Test that DaemonRunDetailResult correctly parses run details.
    #[test]
    fn test_parse_run_detail_for_pipeline_completion() {
        let json = serde_json::json!({
            "run": {
                "id": "run-789",
                "repo_id": "repo-abc",
                "status": "pendingapproval",
                "created_at": 1704067200,
                "completed_at": 1704067260,
                "error": null
            }
        });

        let result: DaemonRunDetailResult = serde_json::from_value(json).unwrap();

        assert_eq!(result.run.id, "run-789");
        assert_eq!(result.run.repo_id, "repo-abc");
        assert_eq!(result.run.status, "pendingapproval");
        assert!(result.run.completed_at.is_some());
        assert!(result.run.error.is_none());
    }

    /// Test parsing a failed pipeline run with error message.
    #[test]
    fn test_parse_failed_pipeline_run() {
        let json = serde_json::json!({
            "run": {
                "id": "run-error",
                "repo_id": "repo-xyz",
                "status": "failed",
                "created_at": 1704067200,
                "completed_at": 1704067230,
                "error": "Pipeline failed: Unable to parse diff"
            }
        });

        let result: DaemonRunDetailResult = serde_json::from_value(json).unwrap();

        assert_eq!(result.run.status, "failed");
        assert!(result.run.error.is_some());
        assert_eq!(
            result.run.error.as_ref().unwrap(),
            "Pipeline failed: Unable to parse diff"
        );
    }

    /// Test that StatusResponse correctly parses repo status with latest run.
    #[test]
    fn test_parse_status_response_with_completed_run() {
        let json = serde_json::json!({
            "repo": {
                "id": "repo-123",
                "working_path": "/path/to/repo",
                "upstream_url": "git@github.com:user/repo.git",
                "gate_path": "/home/user/.airlock/repos/repo-123.git",
                "created_at": 1704000000
            },
            "pending_runs": 1,
            "latest_run": {
                "id": "run-latest",
                "status": "pendingapproval",
                "created_at": 1704067200,
                "completed_at": 1704067260
            },
            "last_sync": {
                "success": true,
                "synced_at": 1704066000,
                "error": null
            }
        });

        let result: DaemonStatusResult = serde_json::from_value(json).unwrap();

        assert_eq!(result.repo.id, "repo-123");
        assert_eq!(result.pending_runs, 1);
        assert!(result.latest_run.is_some());

        let latest_run: RunInfo = result.latest_run.unwrap();
        assert_eq!(latest_run.status, "pendingapproval");
        assert!(
            latest_run.completed_at.is_some(),
            "Completed run should have completed_at"
        );

        assert!(result.last_sync.is_some());
    }
}
