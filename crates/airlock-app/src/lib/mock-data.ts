/**
 * Mock data for browser-only testing of the Airlock UI.
 * This data is used when the app runs outside of Tauri (e.g., via `npm run dev`).
 *
 * This module now uses fixtures generated from a real Airlock database,
 * ensuring mock data matches production serialization exactly.
 *
 * To regenerate fixtures from your database:
 *   cargo run --bin generate-fixtures
 */

import type {
  RepoInfo,
  RunInfo,
  RunDetail,
  StepResultInfo,
  JobResultInfo,
  ArtifactInfo,
  HealthResponse,
  StatusResponse,
  GetConfigResult,
  UpdateConfigResult,
  ApproveStepResult,
  GetRunDiffResult,
  IntentDiffResult,
  IntentTourResult,
} from '../hooks/use-daemon';

// Import generated fixtures
import * as fixtures from './fixtures';

// =============================================================================
// Mock Repositories - Using generated fixtures from database export
// =============================================================================

export const mockRepos: RepoInfo[] = fixtures.getAllRepos().map((r) => ({
  ...r,
  last_sync: r.last_sync ?? null,
}));

// =============================================================================
// Mock Runs - Using generated fixtures from database export
// =============================================================================

// Build runs indexed by repo ID from fixtures
export const mockRuns: Record<string, RunInfo[]> = {};
for (const repo of mockRepos) {
  const runs = fixtures.getRunsForRepo(repo.id);
  mockRuns[repo.id] = runs.map((run) => ({
    ...run,
    repo_id: repo.id,
    error: null,
  })) as RunInfo[];
}

// =============================================================================
// Mock Step Results - Extracted from run details
// =============================================================================

export const mockStepResults: Record<string, StepResultInfo[]> = {};
export const mockJobResults: Record<string, JobResultInfo[]> = {};
export const mockArtifacts: Record<string, ArtifactInfo[]> = {};

// Populate from run details
for (const runs of Object.values(mockRuns)) {
  for (const run of runs) {
    const detail = fixtures.findRunDetail(run.id);
    if (detail) {
      mockStepResults[run.id] = detail.step_results as StepResultInfo[];
      mockJobResults[run.id] = (detail.jobs ?? []) as JobResultInfo[];
      mockArtifacts[run.id] = (detail.artifacts ?? []) as ArtifactInfo[];
    }
  }
}

// =============================================================================
// Mock Config
// =============================================================================

export const mockGlobalConfig: GetConfigResult = {
  global: {
    config_exists: true,
    config_path: '/Users/dev/.config/airlock/config.toml',
    sync: {
      on_fetch: true,
    },
    storage: {
      max_artifact_age_days: 30,
    },
    agent: {
      adapter: 'claude-code',
      model: 'claude-sonnet-4-5-20250929',
      max_turns: null,
    },
  },
};

export const mockRepoConfigs: Record<string, GetConfigResult> = {};
for (const repo of mockRepos) {
  mockRepoConfigs[repo.id] = {
    ...mockGlobalConfig,
    repo: {
      repo_id: repo.id,
      working_path: repo.working_path,
      config_exists: true,
      config_path: `${repo.working_path}/.airlock/workflows/`,
      workflows: [{ filename: 'main.yml', name: 'Main Pipeline' }],
    },
  };
}

// =============================================================================
// Mock Health Response
// =============================================================================

export const mockHealth: HealthResponse = {
  healthy: true,
  version: '0.1.0-dev',
  repo_count: mockRepos.length,
  database_ok: true,
};

// =============================================================================
// Helper Functions
// =============================================================================

export function getRunDetail(runId: string): RunDetail | null {
  const fixtureDetail = fixtures.findRunDetail(runId);
  if (fixtureDetail) {
    return {
      run: {
        ...fixtureDetail.run,
        repo_id: fixtureDetail.run.repo_id,
      } as RunDetail['run'],
      jobs: (fixtureDetail.jobs ?? []) as JobResultInfo[],
      step_results: fixtureDetail.step_results as StepResultInfo[],
      artifacts: (fixtureDetail.artifacts ?? []) as ArtifactInfo[],
    };
  }
  return null;
}

export function getRepoStatus(repoId: string): StatusResponse | null {
  const repo = mockRepos.find((r) => r.id === repoId);
  if (!repo) return null;

  const runs = mockRuns[repoId] || [];
  const latestRun = runs.length > 0 ? runs[0] : null;

  return {
    repo,
    pending_runs: repo.pending_runs,
    latest_run: latestRun,
  };
}

export function getConfig(repoId?: string): GetConfigResult {
  if (repoId && mockRepoConfigs[repoId]) {
    return mockRepoConfigs[repoId];
  }
  return mockGlobalConfig;
}

export function updateConfig(): UpdateConfigResult {
  return {
    success: true,
    global_updated: true,
    repo_updated: false,
    global_config_path: '/Users/dev/.config/airlock/config.toml',
  };
}

// =============================================================================
// Step Approval Helpers
// =============================================================================

export function approveStep(runId: string, jobKey: string, stepName: string): ApproveStepResult {
  console.log(`[Mock] Approving step ${stepName} in job ${jobKey} for run ${runId}`);

  return {
    run_id: runId,
    job_key: jobKey,
    step_name: stepName,
    success: true,
    new_step_status: 'passed',
    pipeline_completed: stepName === 'create-pr',
    paused_at_step: null,
  };
}

export function getRunDiff(runId: string): GetRunDiffResult {
  // Try to get real diff data from fixtures first
  const fixtureDiff = fixtures.getRunDiff(runId);
  if (fixtureDiff) {
    return fixtureDiff;
  }

  // Fallback for runs without diff fixtures
  const detail = fixtures.findRunDetail(runId);
  const branch = detail?.run.branch || 'unknown';
  const baseSha = detail?.run.base_sha || '0000000000000000000000000000000000000000';
  const headSha = detail?.run.head_sha || '0000000000000000000000000000000000000000';

  return {
    run_id: runId,
    branch,
    base_sha: baseSha,
    head_sha: headSha,
    patch: '',
    files_changed: [],
    additions: 0,
    deletions: 0,
  };
}

// =============================================================================
// Intent Helpers
// =============================================================================

export function getIntentDiff(intentId: string): IntentDiffResult {
  return {
    intent_id: intentId,
    hunks: [],
    patch: '',
  };
}

export function getIntentTour(intentId: string): IntentTourResult {
  return {
    intent_id: intentId,
    tour: null,
  };
}

// =============================================================================
// Artifact Content Reading
// =============================================================================

export interface ReadArtifactResult {
  content: string;
  is_binary: boolean;
  total_size: number;
  bytes_read: number;
  offset: number;
}

export function readArtifact(artifactPath: string, offset?: number, limit?: number): ReadArtifactResult {
  // Try to get real content from fixtures
  const fixtureResult = fixtures.readArtifact(artifactPath, offset, limit);
  if (fixtureResult) {
    return fixtureResult;
  }

  // Fallback for artifacts not in fixtures
  const readOffset = offset ?? 0;
  const defaultContent = `# Artifact Content\n\nThis artifact was not found in the fixtures.\n\nPath: ${artifactPath}`;
  const slicedContent = limit ? defaultContent.slice(readOffset, readOffset + limit) : defaultContent.slice(readOffset);

  return {
    content: slicedContent,
    is_binary: false,
    total_size: defaultContent.length,
    bytes_read: slicedContent.length,
    offset: readOffset,
  };
}
