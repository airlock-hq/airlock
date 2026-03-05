/**
 * Tauri API wrapper with mock support for browser-only testing.
 *
 * When running in a browser (via `npm run dev`), this module provides
 * mock responses for all Tauri IPC commands, allowing UI development
 * without the full Tauri backend.
 */

import * as mockData from './mock-data';

// Check if we're running inside Tauri (v2 uses __TAURI_INTERNALS__)
export const isTauri = '__TAURI__' in window || '__TAURI_INTERNALS__' in window;

// Simulated delay for mock responses (ms)
const MOCK_DELAY = 150;

/**
 * Simulates network delay for more realistic mock behavior.
 */
function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Mock implementation of Tauri's invoke function.
 * Routes commands to appropriate mock data handlers.
 */
async function mockInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  await delay(MOCK_DELAY);

  switch (cmd) {
    case 'check_health':
      return mockData.mockHealth as T;

    case 'list_repos':
      return mockData.mockRepos as T;

    case 'get_repo_status': {
      const repoId = args?.repoId as string;
      const status = mockData.getRepoStatus(repoId);
      if (!status) {
        throw new Error(`Repository not found: ${repoId}`);
      }
      return status as T;
    }

    case 'get_runs': {
      const repoId = args?.repoId as string;
      const limit = args?.limit as number | undefined;
      let runs = mockData.mockRuns[repoId] || [];
      if (limit) {
        runs = runs.slice(0, limit);
      }
      return runs as T;
    }

    case 'get_run_detail': {
      const runId = args?.runId as string;
      const detail = mockData.getRunDetail(runId);
      if (!detail) {
        throw new Error(`Run not found: ${runId}`);
      }
      return detail as T;
    }

    case 'get_intent_diff': {
      const intentId = args?.intentId as string;
      return mockData.getIntentDiff(intentId) as T;
    }

    case 'get_intent_tour': {
      const intentId = args?.intentId as string;
      return mockData.getIntentTour(intentId) as T;
    }

    case 'approve_intent': {
      const intentId = args?.intentId as string;
      console.log(`[Mock] Approve intent ${intentId}`);
      return {
        intent_id: intentId,
        success: true,
        new_status: 'approved',
      } as T;
    }

    case 'reject_intent': {
      const intentId = args?.intentId as string;
      const reason = args?.reason as string | undefined;
      console.log(`[Mock] Reject intent ${intentId}`, reason ? `reason: ${reason}` : '');
      return {
        intent_id: intentId,
        success: true,
        new_status: 'rejected',
      } as T;
    }

    case 'sync_repo': {
      const repoId = args?.repoId as string;
      console.log(`[Mock] Sync repo ${repoId}`);
      return true as T;
    }

    case 'sync_all': {
      console.log('[Mock] Sync all repos');
      return [mockData.mockRepos.length, 0] as T;
    }

    case 'update_intent_description': {
      const intentId = args?.intentId as string;
      const description = args?.description as string;
      console.log(`[Mock] Update intent ${intentId} description: ${description}`);
      return description as T;
    }

    case 'reprocess_run': {
      const runId = args?.runId as string;
      console.log(`[Mock] Reprocess run ${runId}`);
      return true as T;
    }

    case 'approve_step': {
      const runId = args?.runId as string;
      const jobKey = args?.jobKey as string;
      const stepName = args?.stepName as string;
      console.log(`[Mock] Approve step ${stepName} in job ${jobKey} for run ${runId}`);
      return mockData.approveStep(runId, jobKey, stepName) as T;
    }

    case 'get_run_diff': {
      const runId = args?.runId as string;
      console.log(`[Mock] Get diff for run ${runId}`);
      return mockData.getRunDiff(runId) as T;
    }

    case 'get_config': {
      const repoId = args?.repoId as string | undefined;
      return mockData.getConfig(repoId) as T;
    }

    case 'update_config': {
      console.log('[Mock] Update config:', args);
      return mockData.updateConfig() as T;
    }

    case 'apply_patches': {
      const runId = args?.runId as string;
      const patchPaths = args?.patchPaths as string[];
      console.log(`[Mock] Apply patches for run ${runId}:`, patchPaths);
      return {
        run_id: runId,
        success: true,
        applied_count: patchPaths?.length ?? 0,
        new_head_sha: 'mock-sha-' + Date.now(),
        error: null,
        patch_errors: [],
      } as T;
    }

    case 'read_artifact': {
      const artifactPath = args?.artifactPath as string;
      console.log(`[Mock] Read artifact: ${artifactPath}`);
      return mockData.readArtifact(artifactPath) as T;
    }

    case 'show_window':
      return undefined as T;

    default:
      throw new Error(`Unknown command: ${cmd}`);
  }
}

/**
 * Universal invoke function that uses Tauri's invoke when available,
 * or falls back to mock data when running in a browser.
 */
export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) {
    // Dynamically import Tauri's invoke to avoid errors in browser
    const { invoke: tauriInvoke } = await import('@tauri-apps/api/core');
    return tauriInvoke<T>(cmd, args);
  } else {
    console.log(`[Mock Mode] invoke("${cmd}", ${JSON.stringify(args)})`);
    return mockInvoke<T>(cmd, args);
  }
}
