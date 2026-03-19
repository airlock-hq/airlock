import { invoke } from '../lib/tauri';
import { useCallback } from 'react';
import { useRefreshOnEvents, AIRLOCK_EVENTS } from './use-airlock-events';
import { useDaemonQuery } from './use-daemon-query';

// Types matching the Tauri backend
export interface RepoInfo {
  id: string;
  working_path: string;
  upstream_url: string;
  gate_path: string;
  created_at: number;
  last_sync: number | null;
  pending_runs: number;
}

export interface RunInfo {
  id: string;
  repo_id: string;
  status: string;
  /** Branch being pushed */
  branch?: string;
  /** Currently executing step name (for running pipelines) */
  current_step?: string | null;
  /** Workflow file that triggered this run */
  workflow_file?: string;
  /** Workflow display name */
  workflow_name?: string;
  created_at: number;
  /** When the run was last updated */
  updated_at?: number;
  completed_at: number | null;
  error: string | null;
}

export interface DiffHunkInfo {
  id: string;
  file_path: string;
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  additions: number;
  deletions: number;
  content: string;
  language: string | null;
}

export interface IntentDiffResult {
  intent_id: string;
  hunks: DiffHunkInfo[];
  patch: string;
}

export interface IntentTourResult {
  intent_id: string;
  tour: TourInfo | null;
}

export interface TourInfo {
  title: string;
  overview: string;
  steps: TourStepInfo[];
  estimated_minutes: number;
}

export interface TourStepInfo {
  step_number: number;
  title: string;
  explanation: string;
  file: string;
  start_line: number;
  end_line: number;
  code_snippet: string;
  annotations: LineAnnotationInfo[];
  deep_dive: string | null;
}

export interface LineAnnotationInfo {
  line_offset: number;
  text: string;
  annotation_type: string;
}

export interface JobResultInfo {
  /** Job result ID */
  id: string;
  /** Job key from the workflow YAML (e.g., "default", "lint", "test") */
  job_key: string;
  /** Display name for the job */
  name?: string;
  /** Job status: "pending", "running", "passed", "failed", "skipped", "awaiting_approval" */
  status: string;
  /** Topological order for display */
  job_order: number;
  /** When the job started (Unix timestamp) */
  started_at?: number;
  /** When the job completed (Unix timestamp) */
  completed_at?: number;
  /** Error message if the job failed */
  error?: string;
}

export interface StepResultInfo {
  /** Step result ID */
  id?: string;
  /** Job result ID this step belongs to */
  job_id?: string;
  /** Job key this step belongs to */
  job_key?: string;
  /** Step name (e.g., "describe", "test", "push", "create-pr") */
  step: string;
  /** Step status: "pending", "running", "passed", "failed", "skipped", "awaiting_approval" */
  status: string;
  /** Exit code of the step command (if executed) */
  exit_code?: number;
  /** Duration in milliseconds (if completed) */
  duration_ms?: number;
  /** Error message if the step failed */
  error?: string;
  /** When the step started (Unix timestamp) */
  started_at?: number;
  /** When the step completed (Unix timestamp) */
  completed_at?: number;
}

export interface ArtifactInfo {
  name: string;
  path: string;
  artifact_type: string;
  size_bytes: number;
  created_at: number;
}

export interface RunDetail {
  run: RunInfo & {
    /** Branch being pushed */
    branch?: string;
    /** Base commit SHA */
    base_sha?: string;
    /** Head commit SHA */
    head_sha?: string;
    /** Currently executing step */
    current_step?: string | null;
    /** Workflow file that triggered this run */
    workflow_file?: string;
    /** Workflow display name */
    workflow_name?: string;
    /** When the run was last updated */
    updated_at?: number;
  };
  jobs: JobResultInfo[];
  step_results: StepResultInfo[];
  artifacts: ArtifactInfo[];
}

export interface StatusResponse {
  repo: RepoInfo;
  pending_runs: number;
  latest_run: RunInfo | null;
}

export interface HealthResponse {
  healthy: boolean;
  version: string;
  repo_count: number;
  database_ok: boolean;
}

// Hook for checking daemon health
export function useDaemonHealth() {
  const fetcher = useCallback(() => invoke<HealthResponse>('check_health'), []);
  const {
    data: health,
    error,
    loading,
    refresh,
  } = useDaemonQuery<HealthResponse | null>(fetcher, null, {
    resetOnError: true,
    pollingIntervalMs: 30000,
  });
  return { health, error, loading, refresh };
}

// Hook for listing repositories
export function useRepos() {
  const fetcher = useCallback(() => invoke<RepoInfo[]>('list_repos'), []);
  const { data: repos, error, loading, refresh } = useDaemonQuery<RepoInfo[]>(fetcher, []);

  // Auto-refresh when runs are created/completed (affects pending_runs count)
  useRefreshOnEvents(refresh, {
    events: [AIRLOCK_EVENTS.RUN_CREATED, AIRLOCK_EVENTS.RUN_COMPLETED],
  });

  return { repos, error, loading, refresh };
}

// Hook for getting repository status
export function useRepoStatus(repoId: string | null) {
  const fetcher = useCallback(async () => {
    if (!repoId) return undefined;
    return invoke<StatusResponse>('get_repo_status', { repoId });
  }, [repoId]);
  const {
    data: status,
    error,
    loading,
    refresh,
  } = useDaemonQuery<StatusResponse | null>(fetcher, null, {
    initialLoading: false,
  });

  // Auto-refresh on run events for this repo
  useRefreshOnEvents(refresh, {
    repoId,
    events: [AIRLOCK_EVENTS.RUN_CREATED, AIRLOCK_EVENTS.RUN_UPDATED, AIRLOCK_EVENTS.RUN_COMPLETED],
  });

  return { status, error, loading, refresh };
}

// Hook for getting runs for a repository
export function useRuns(repoId: string | null, limit?: number) {
  const fetcher = useCallback(async () => {
    if (!repoId) return undefined;
    return invoke<RunInfo[]>('get_runs', { repoId, limit });
  }, [repoId, limit]);
  const {
    data: runs,
    error,
    loading,
    refresh,
  } = useDaemonQuery<RunInfo[]>(fetcher, [], {
    initialLoading: false,
  });

  // Auto-refresh on run-level events only (daemon emits RunUpdated on job transitions)
  useRefreshOnEvents(refresh, {
    repoId,
    events: [
      AIRLOCK_EVENTS.RUN_CREATED,
      AIRLOCK_EVENTS.RUN_UPDATED,
      AIRLOCK_EVENTS.RUN_COMPLETED,
      AIRLOCK_EVENTS.RUN_SUPERSEDED,
    ],
  });

  return { runs, error, loading, refresh };
}

// Hook for getting run detail
export function useRunDetail(runId: string | null) {
  const fetcher = useCallback(async () => {
    if (!runId) return undefined;
    return invoke<RunDetail>('get_run_detail', { runId });
  }, [runId]);
  const {
    data: detail,
    error,
    loading,
    refresh,
  } = useDaemonQuery<RunDetail | null>(fetcher, null, {
    initialLoading: false,
  });

  // Auto-refresh on job, step, and run events for this run
  useRefreshOnEvents(refresh, {
    runId,
    events: [
      AIRLOCK_EVENTS.RUN_UPDATED,
      AIRLOCK_EVENTS.JOB_STARTED,
      AIRLOCK_EVENTS.JOB_COMPLETED,
      AIRLOCK_EVENTS.STEP_STARTED,
      AIRLOCK_EVENTS.STEP_COMPLETED,
      AIRLOCK_EVENTS.RUN_COMPLETED,
      AIRLOCK_EVENTS.RUN_SUPERSEDED,
    ],
  });

  return { detail, error, loading, refresh };
}

// Functions for actions (not hooks)
export async function getIntentDiff(intentId: string): Promise<IntentDiffResult> {
  return invoke<IntentDiffResult>('get_intent_diff', { intentId });
}

export async function getIntentTour(intentId: string): Promise<IntentTourResult> {
  return invoke<IntentTourResult>('get_intent_tour', { intentId });
}

// =============================================================================
// Intent Approve/Reject Types and Functions
// =============================================================================

export interface ApproveIntentResult {
  intent_id: string;
  success: boolean;
  new_status: string;
}

export interface RejectIntentResult {
  intent_id: string;
  success: boolean;
  new_status: string;
}

// =============================================================================
// Step Approve/Reject Types and Functions (workflow/job/step pipeline)
// =============================================================================

export interface ApproveStepResult {
  run_id: string;
  job_key: string;
  step_name: string;
  success: boolean;
  new_step_status: string;
  /** Whether the pipeline resumed and completed */
  pipeline_completed: boolean;
  /** Whether the pipeline is paused at another step awaiting approval */
  paused_at_step: string | null;
}

export interface CommitDiffInfo {
  sha: string;
  message: string;
  author: string;
  timestamp: number;
  patch: string;
  files_changed: string[];
  additions: number;
  deletions: number;
}

export interface GetRunDiffResult {
  run_id: string;
  branch: string;
  base_sha: string;
  head_sha: string;
  /** Full unified diff patch string */
  patch: string;
  /** Files changed in this run */
  files_changed: string[];
  /** Number of lines added */
  additions: number;
  /** Number of lines deleted */
  deletions: number;
  /** Per-commit diff information (empty for single-commit pushes) */
  commits?: CommitDiffInfo[];
}

export async function approveStep(runId: string, jobKey: string, stepName: string): Promise<ApproveStepResult> {
  return invoke<ApproveStepResult>('approve_step', { runId, jobKey, stepName });
}

export async function getRunDiff(runId: string): Promise<GetRunDiffResult> {
  return invoke<GetRunDiffResult>('get_run_diff', { runId });
}

export async function approveIntent(intentId: string): Promise<ApproveIntentResult> {
  return invoke<ApproveIntentResult>('approve_intent', { intentId });
}

export async function rejectIntent(intentId: string, reason?: string): Promise<RejectIntentResult> {
  return invoke<RejectIntentResult>('reject_intent', { intentId, reason });
}

export async function syncRepo(repoId: string): Promise<boolean> {
  return invoke<boolean>('sync_repo', { repoId });
}

export async function syncAll(): Promise<[number, number]> {
  return invoke<[number, number]>('sync_all');
}

export async function updateIntentDescription(intentId: string, description: string): Promise<string> {
  return invoke<string>('update_intent_description', { intentId, description });
}

export async function reprocessRun(runId: string): Promise<boolean> {
  return invoke<boolean>('reprocess_run', { runId });
}

export async function retryJob(runId: string, jobKey: string): Promise<boolean> {
  return invoke<boolean>('retry_job', { runId, jobKey });
}

export async function cancelRun(runId: string): Promise<boolean> {
  return invoke<boolean>('cancel_run', { runId });
}

// =============================================================================
// Apply Patches Types and Functions
// =============================================================================

export interface PatchError {
  path: string;
  error: string;
}

export interface ApplyPatchesResult {
  run_id: string;
  success: boolean;
  applied_count: number;
  new_head_sha: string | null;
  error: string | null;
  patch_errors: PatchError[];
}

export async function applyPatches(runId: string, patchPaths: string[]): Promise<ApplyPatchesResult> {
  return invoke<ApplyPatchesResult>('apply_patches', { runId, patchPaths });
}

// =============================================================================
// Artifact Reading Types and Functions
// =============================================================================

export interface ReadArtifactResult {
  content: string;
  is_binary: boolean;
  /** Total size of the file in bytes */
  total_size: number;
  /** Number of bytes read in this response */
  bytes_read: number;
  /** Offset from which content was read */
  offset: number;
}

export async function readArtifact(artifactPath: string, offset?: number, limit?: number): Promise<ReadArtifactResult> {
  return invoke<ReadArtifactResult>('read_artifact', {
    artifactPath,
    offset,
    limit,
  });
}

// =============================================================================
// Configuration Types
// =============================================================================

export interface SyncConfigInfo {
  on_fetch: boolean;
}

export interface StorageConfigInfo {
  max_artifact_age_days: number;
}

export interface AgentConfigInfo {
  adapter: string;
  model: string | null;
  max_turns: number | null;
}

export interface GlobalConfigInfo {
  config_exists: boolean;
  config_path: string;
  sync: SyncConfigInfo;
  storage: StorageConfigInfo;
  agent: AgentConfigInfo;
}

export interface WorkflowFileInfo {
  filename: string;
  name: string | null;
}

export interface RepoConfigInfo {
  repo_id: string;
  working_path: string;
  config_exists: boolean;
  config_path: string;
  workflows: WorkflowFileInfo[];
}

export interface GetConfigResult {
  global: GlobalConfigInfo;
  repo?: RepoConfigInfo;
}

export interface SyncConfigUpdate {
  on_fetch?: boolean;
}

export interface StorageConfigUpdate {
  max_artifact_age_days?: number;
}

export interface GlobalConfigUpdate {
  sync?: SyncConfigUpdate;
  storage?: StorageConfigUpdate;
}

export interface RepoConfigUpdate {
  repo_id: string;
}

export interface UpdateConfigResult {
  success: boolean;
  global_updated: boolean;
  repo_updated: boolean;
  global_config_path?: string;
  repo_config_path?: string;
}

// =============================================================================
// Configuration Functions
// =============================================================================

export async function getConfig(repoId?: string): Promise<GetConfigResult> {
  return invoke<GetConfigResult>('get_config', { repoId });
}

export async function updateConfig(global?: GlobalConfigUpdate, repo?: RepoConfigUpdate): Promise<UpdateConfigResult> {
  return invoke<UpdateConfigResult>('update_config', { global, repo });
}

// Hook for getting all runs across all repos
export function useAllRuns(limit?: number) {
  const fetcher = useCallback(async () => {
    const repos = await invoke<RepoInfo[]>('list_repos');
    const allRunsPromises = repos.map(async (repo) => {
      const repoRuns = await invoke<RunInfo[]>('get_runs', {
        repoId: repo.id,
        limit: limit ?? 10,
      });
      const repoName = getRepoNameFromUrl(repo.upstream_url);
      return repoRuns.map((run) => ({ ...run, repo_name: repoName }));
    });
    const allRunsArrays = await Promise.all(allRunsPromises);
    const allRuns = allRunsArrays.flat();
    allRuns.sort((a, b) => b.created_at - a.created_at);
    return limit ? allRuns.slice(0, limit) : allRuns;
  }, [limit]);

  const { data: runs, error, loading, refresh } = useDaemonQuery<(RunInfo & { repo_name: string })[]>(fetcher, []);

  // Auto-refresh on run-level events only (daemon emits RunUpdated on job transitions)
  useRefreshOnEvents(refresh, {
    events: [
      AIRLOCK_EVENTS.RUN_CREATED,
      AIRLOCK_EVENTS.RUN_UPDATED,
      AIRLOCK_EVENTS.RUN_COMPLETED,
      AIRLOCK_EVENTS.RUN_SUPERSEDED,
    ],
  });

  return { runs, error, loading, refresh };
}

// Helper to extract repo name from URL
export function getRepoNameFromUrl(url: string): string {
  // Handle SSH URLs: git@github.com:user/repo.git
  const sshMatch = url.match(/[:/]([^/]+\/[^/.]+)(\.git)?$/);
  if (sshMatch) return sshMatch[1];

  // Handle HTTPS URLs: https://github.com/user/repo.git
  const httpsMatch = url.match(/\/([^/]+\/[^/.]+)(\.git)?$/);
  if (httpsMatch) return httpsMatch[1];

  return url;
}

// Hook for getting configuration
export function useConfig(repoId?: string) {
  const fetcher = useCallback(() => invoke<GetConfigResult>('get_config', { repoId }), [repoId]);
  const {
    data: config,
    error,
    loading,
    refresh,
  } = useDaemonQuery<GetConfigResult | null>(fetcher, null, {
    resetOnError: true,
  });
  return { config, error, loading, refresh };
}
