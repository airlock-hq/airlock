import { useState, useEffect, useCallback, useRef } from 'react';

export interface UseDaemonQueryOptions {
  /** Initial loading state. Default: true. Set to false for conditional hooks that may skip. */
  initialLoading?: boolean;
  /** If true, reset data to defaultValue on error. Default: false */
  resetOnError?: boolean;
  /** Poll interval in ms for periodic re-fetching */
  pollingIntervalMs?: number;
}

/**
 * Generic hook for querying daemon data with loading/error state management.
 *
 * @param fetcher - Memoized async function (useCallback). Return undefined to skip the fetch.
 * @param defaultValue - Initial value for the data state.
 * @param options - Optional configuration.
 */
export function useDaemonQuery<T>(
  fetcher: () => Promise<T | undefined>,
  defaultValue: T,
  options: UseDaemonQueryOptions = {}
): { data: T; error: string | null; loading: boolean; refresh: () => Promise<void> } {
  const { initialLoading = true, resetOnError = false, pollingIntervalMs } = options;

  const [data, setData] = useState<T>(defaultValue);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(initialLoading);
  const hasLoaded = useRef(false);
  const resetOnErrorRef = useRef(resetOnError);
  const defaultValueRef = useRef(defaultValue);

  const refresh = useCallback(async () => {
    try {
      if (!hasLoaded.current) setLoading(true);
      const result = await fetcher();
      if (result !== undefined) {
        setData(result);
        setError(null);
        hasLoaded.current = true;
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      if (resetOnErrorRef.current) setData(defaultValueRef.current);
    } finally {
      setLoading(false);
    }
  }, [fetcher]);

  useEffect(() => {
    refresh();
    if (pollingIntervalMs) {
      const interval = setInterval(refresh, pollingIntervalMs);
      return () => clearInterval(interval);
    }
  }, [refresh, pollingIntervalMs]);

  return { data, error, loading, refresh };
}
