/**
 * React hooks for listening to Airlock daemon events.
 *
 * These hooks provide a way to subscribe to real-time events from the daemon
 * and trigger refreshes when relevant events occur.
 */

import { useEffect, useCallback, useRef } from 'react';
import { isTauri } from '@/lib/tauri';

// Event types matching the Rust AirlockEvent enum
export interface RunCreatedEvent {
  repo_id: string;
  run_id: string;
  branch: string;
}

export interface RunUpdatedEvent {
  repo_id: string;
  run_id: string;
  status: string;
}

export interface JobStartedEvent {
  repo_id: string;
  run_id: string;
  job_key: string;
}

export interface JobCompletedEvent {
  repo_id: string;
  run_id: string;
  job_key: string;
  status: string;
}

export interface StepStartedEvent {
  repo_id: string;
  run_id: string;
  job_key: string;
  step_name: string;
}

export interface StepCompletedEvent {
  repo_id: string;
  run_id: string;
  job_key: string;
  step_name: string;
  status: string;
  branch: string;
}

export interface RunCompletedEvent {
  repo_id: string;
  run_id: string;
  success: boolean;
  branch: string;
}

export interface LogChunkEvent {
  repo_id: string;
  run_id: string;
  job_key: string;
  step_name: string;
  stream: 'stdout' | 'stderr';
  content: string;
}

// Union type for all events
export type AirlockEventPayload =
  | ({ type: 'run_created' } & RunCreatedEvent)
  | ({ type: 'run_updated' } & RunUpdatedEvent)
  | ({ type: 'job_started' } & JobStartedEvent)
  | ({ type: 'job_completed' } & JobCompletedEvent)
  | ({ type: 'step_started' } & StepStartedEvent)
  | ({ type: 'step_completed' } & StepCompletedEvent)
  | ({ type: 'run_completed' } & RunCompletedEvent)
  | ({ type: 'log_chunk' } & LogChunkEvent);

// Event names for specific event types
export const AIRLOCK_EVENTS = {
  ALL: 'airlock://event',
  RUN_CREATED: 'airlock://run-created',
  RUN_UPDATED: 'airlock://run-updated',
  JOB_STARTED: 'airlock://job-started',
  JOB_COMPLETED: 'airlock://job-completed',
  STEP_STARTED: 'airlock://step-started',
  STEP_COMPLETED: 'airlock://step-completed',
  RUN_COMPLETED: 'airlock://run-completed',
  RUN_SUPERSEDED: 'airlock://run-superseded',
  LOG_CHUNK: 'airlock://log-chunk',
} as const;

/**
 * Hook for listening to a specific Airlock event.
 *
 * @param eventName - The event name to listen for
 * @param callback - Function called when the event is received
 *
 * @example
 * ```tsx
 * useAirlockEvent('airlock://run-created', (event) => {
 *   console.log('New run created:', event.payload.run_id);
 * });
 * ```
 */
export function useAirlockEvent<T>(eventName: string, callback: (payload: T) => void): void {
  const callbackRef = useRef(callback);
  callbackRef.current = callback;

  useEffect(() => {
    // Skip in browser mode - events only work in Tauri
    if (!isTauri) {
      return;
    }

    let unlisten: (() => void) | undefined;

    // Dynamically import Tauri's event API
    import('@tauri-apps/api/event').then(({ listen }) => {
      listen<T>(eventName, (event) => {
        callbackRef.current(event.payload);
      }).then((unlistenFn) => {
        unlisten = unlistenFn;
      });
    });

    return () => {
      unlisten?.();
    };
  }, [eventName]);
}

/**
 * Options for useRefreshOnEvents hook.
 */
export interface RefreshOnEventsOptions {
  /** Filter events by repo_id */
  repoId?: string | null;
  /** Filter events by run_id */
  runId?: string | null;
  /** Filter events by job_key */
  jobKey?: string | null;
  /** Filter events by step_name */
  stepName?: string | null;
  /** Event types to listen for (defaults to all refresh-worthy events) */
  events?: string[];
  /** Debounce time in ms (default: 100) */
  debounceMs?: number;
}

/**
 * Hook that triggers a refresh callback when relevant events occur.
 *
 * This hook is designed to be used with data fetching hooks to automatically
 * refresh data when the daemon reports relevant changes.
 *
 * @param refresh - The refresh function to call
 * @param options - Options to filter which events trigger refresh
 *
 * @example
 * ```tsx
 * const { runs, refresh } = useRuns(repoId);
 *
 * // Auto-refresh when runs change for this repo
 * useRefreshOnEvents(refresh, { repoId });
 * ```
 */
export function useRefreshOnEvents(refresh: () => void, options: RefreshOnEventsOptions = {}): void {
  const {
    repoId,
    runId,
    jobKey,
    stepName,
    events = [
      AIRLOCK_EVENTS.RUN_CREATED,
      AIRLOCK_EVENTS.RUN_UPDATED,
      AIRLOCK_EVENTS.RUN_COMPLETED,
      AIRLOCK_EVENTS.JOB_COMPLETED,
      AIRLOCK_EVENTS.STEP_COMPLETED,
    ],
    debounceMs = 100,
  } = options;

  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;

  const debounceTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Debounced refresh
  const debouncedRefresh = useCallback(() => {
    if (debounceTimer.current) {
      clearTimeout(debounceTimer.current);
    }
    debounceTimer.current = setTimeout(() => {
      refreshRef.current();
    }, debounceMs);
  }, [debounceMs]);

  // Filter function to check if event matches our criteria
  const shouldRefresh = useCallback(
    (payload: Record<string, unknown>) => {
      // Check repo_id filter
      if (repoId && payload.repo_id !== repoId) {
        return false;
      }
      // Check run_id filter
      if (runId && payload.run_id !== runId) {
        return false;
      }
      // Check job_key filter
      if (jobKey && payload.job_key !== jobKey) {
        return false;
      }
      // Check step_name filter
      if (stepName && payload.step_name !== stepName) {
        return false;
      }
      return true;
    },
    [repoId, runId, jobKey, stepName]
  );

  useEffect(() => {
    // In browser mode, simulate events with periodic polling
    if (!isTauri) {
      const interval = setInterval(() => {
        refreshRef.current();
      }, 3000);
      return () => clearInterval(interval);
    }

    const unlistenPromises: Promise<() => void>[] = [];

    // Listen to each event type
    import('@tauri-apps/api/event').then(({ listen }) => {
      for (const eventName of events) {
        const promise = listen<Record<string, unknown>>(eventName, (event) => {
          if (shouldRefresh(event.payload)) {
            debouncedRefresh();
          }
        });
        unlistenPromises.push(promise);
      }
    });

    return () => {
      // Cleanup: unlisten from all events
      Promise.all(unlistenPromises).then((unlistenFns) => {
        unlistenFns.forEach((unlisten) => unlisten());
      });

      // Clear any pending debounce
      if (debounceTimer.current) {
        clearTimeout(debounceTimer.current);
      }
    };
  }, [events, shouldRefresh, debouncedRefresh]);
}

/**
 * Hook for listening to log chunk events for a specific step in a job.
 *
 * This hook is optimized for streaming log output from a running step.
 *
 * @param repoId - Repository ID to filter
 * @param runId - Run ID to filter
 * @param jobKey - Job key to filter
 * @param stepName - Step name to filter
 * @param onChunk - Callback when a log chunk is received
 */
export function useLogChunkEvents(
  repoId: string,
  runId: string,
  jobKey: string,
  stepName: string,
  onChunk: (chunk: LogChunkEvent) => void
): void {
  const callbackRef = useRef(onChunk);
  callbackRef.current = onChunk;

  useEffect(() => {
    // Skip in browser mode
    if (!isTauri) {
      return;
    }

    let unlisten: (() => void) | undefined;

    import('@tauri-apps/api/event').then(({ listen }) => {
      listen<LogChunkEvent>(AIRLOCK_EVENTS.LOG_CHUNK, (event) => {
        const chunk = event.payload;
        // Filter by repo, run, job, and step
        if (
          chunk.repo_id === repoId &&
          chunk.run_id === runId &&
          chunk.job_key === jobKey &&
          chunk.step_name === stepName
        ) {
          callbackRef.current(chunk);
        }
      }).then((unlistenFn) => {
        unlisten = unlistenFn;
      });
    });

    return () => {
      unlisten?.();
    };
  }, [repoId, runId, jobKey, stepName]);
}
