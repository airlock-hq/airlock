//! Run-related IPC methods.

use super::error::IpcError;
use super::types::{DaemonGetRunsResult, DaemonReprocessRunResult, DaemonRunDetailResult};
use super::IpcClient;
use crate::{ApplyPatchesResult, ApproveStepResult, GetRunDiffResult, RunDetail, RunInfo};

impl IpcClient {
    /// Get runs for a repository
    pub async fn get_runs(
        &self,
        repo_id: &str,
        limit: Option<u32>,
    ) -> Result<Vec<RunInfo>, IpcError> {
        let params = match limit {
            Some(l) => serde_json::json!({ "repo_id": repo_id, "limit": l }),
            None => serde_json::json!({ "repo_id": repo_id }),
        };

        let result = self.send_request("get_runs", params).await?;
        let daemon_result: DaemonGetRunsResult = serde_json::from_value(result)?;
        Ok(daemon_result.runs)
    }

    /// Get run detail
    pub async fn get_run_detail(&self, run_id: &str) -> Result<RunDetail, IpcError> {
        let result = self
            .send_request("get_run_detail", serde_json::json!({ "run_id": run_id }))
            .await?;

        let daemon_result: DaemonRunDetailResult = serde_json::from_value(result)?;

        Ok(RunDetail {
            run: RunInfo {
                id: daemon_result.run.id,
                repo_id: Some(daemon_result.run.repo_id),
                status: daemon_result.run.status,
                branch: daemon_result.run.branch,
                base_sha: daemon_result.run.base_sha,
                head_sha: daemon_result.run.head_sha,
                current_step: daemon_result.run.current_step,
                created_at: daemon_result.run.created_at,
                updated_at: daemon_result.run.updated_at,
                completed_at: daemon_result.run.completed_at,
                error: daemon_result.run.error,
            },
            jobs: daemon_result.jobs,
            step_results: daemon_result.step_results,
            artifacts: daemon_result.artifacts,
        })
    }

    /// Reprocess a run (re-run the full pipeline)
    pub async fn reprocess_run(&self, run_id: &str) -> Result<bool, IpcError> {
        let result = self
            .send_request("reprocess_run", serde_json::json!({ "run_id": run_id }))
            .await?;

        let daemon_result: DaemonReprocessRunResult = serde_json::from_value(result)?;
        Ok(daemon_result.success)
    }

    /// Approve a step (resume pipeline execution)
    pub async fn approve_step(
        &self,
        run_id: &str,
        job_key: &str,
        step_name: &str,
    ) -> Result<ApproveStepResult, IpcError> {
        let result = self
            .send_request(
                "approve_step",
                serde_json::json!({ "run_id": run_id, "job_key": job_key, "step_name": step_name }),
            )
            .await?;

        Ok(serde_json::from_value(result)?)
    }

    /// Get the diff for a run
    pub async fn get_run_diff(&self, run_id: &str) -> Result<GetRunDiffResult, IpcError> {
        let result = self
            .send_request("get_run_diff", serde_json::json!({ "run_id": run_id }))
            .await?;

        Ok(serde_json::from_value(result)?)
    }

    /// Apply selected patches to a run
    pub async fn apply_patches(
        &self,
        run_id: &str,
        patch_paths: &[String],
    ) -> Result<ApplyPatchesResult, IpcError> {
        let result = self
            .send_request(
                "apply_patches",
                serde_json::json!({ "run_id": run_id, "patch_paths": patch_paths }),
            )
            .await?;

        Ok(serde_json::from_value(result)?)
    }
}
