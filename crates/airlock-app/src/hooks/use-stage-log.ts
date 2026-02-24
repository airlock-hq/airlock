import { useState, useEffect, useCallback, useRef } from 'react';
import { readArtifact } from './use-daemon';
import { isTauri } from '@/lib/tauri';
import { homeDir } from '@tauri-apps/api/path';
import { useLogChunkEvents } from './use-airlock-events';
import type { LogChunkEvent } from './use-airlock-events';

// Mock log content for browser testing
const mockStageLogs: Record<string, { stdout: string; stderr: string }> = {
  describe: {
    stdout: `[2024-01-15 10:30:00] Starting describe stage...
[2024-01-15 10:30:01] Analyzing commit: abc1234
[2024-01-15 10:30:02] Branch: feature/add-auth
[2024-01-15 10:30:03] Files changed: 5
[2024-01-15 10:30:04]   - src/middleware/auth.ts (new)
[2024-01-15 10:30:04]   - src/types/user.ts (new)
[2024-01-15 10:30:04]   - src/config/auth.ts (new)
[2024-01-15 10:30:04]   - src/routes/index.ts (modified)
[2024-01-15 10:30:04]   - package.json (modified)
[2024-01-15 10:30:05] Generating description using Claude...
[2024-01-15 10:30:07] Description generated successfully
[2024-01-15 10:30:08] Stage completed in 8.2s
`,
    stderr: '',
  },
  test: {
    stdout: `[2024-01-15 10:30:10] Starting test stage...
[2024-01-15 10:30:11] Running: npm test

> airlock@0.1.0 test
> jest --passWithNoTests

PASS src/middleware/auth.test.ts
  AuthMiddleware
    ✓ should validate JWT token (45ms)
    ✓ should reject invalid token (12ms)
    ✓ should handle expired token (8ms)
    ✓ should check required roles (15ms)

PASS src/config/auth.test.ts
  AuthConfig
    ✓ should load configuration (5ms)
    ✓ should use environment variables (3ms)

Test Suites: 2 passed, 2 total
Tests:       6 passed, 6 total
Snapshots:   0 total
Time:        3.245s
Ran all test suites.

[2024-01-15 10:30:45] All tests passed
[2024-01-15 10:30:45] Stage completed in 35.1s
`,
    stderr: `[WARN] Deprecation: Jest's "moduleNameMapper" has a deprecated config.
[WARN] Consider updating to the new format.
[DEBUG] Coverage threshold not met: 78% < 80%
`,
  },
  push: {
    stdout: `[2024-01-15 10:35:00] Starting push stage...
[2024-01-15 10:35:01] Creating branch: airlock/feature-add-auth
[2024-01-15 10:35:02] Pushing to origin...
To github.com:example/repo.git
 * [new branch]      airlock/feature-add-auth -> airlock/feature-add-auth
[2024-01-15 10:35:03] Branch pushed successfully
[2024-01-15 10:35:03] Stage completed in 3.2s
`,
    stderr: '',
  },
  'create-pr': {
    stdout: `[2024-01-15 10:35:05] Starting create-pr stage...
[2024-01-15 10:35:06] Creating pull request...
[2024-01-15 10:35:08] PR created: https://github.com/example/repo/pull/42
[2024-01-15 10:35:08] Title: Add user authentication middleware
[2024-01-15 10:35:08] Stage completed in 2.8s
`,
    stderr: '',
  },
};

// Mock running stage output that grows over time
const mockRunningOutput = [
  '[2024-01-15 10:40:00] Starting test stage...',
  '[2024-01-15 10:40:01] Running: npm test',
  '',
  '> airlock@0.1.0 test',
  '> jest --passWithNoTests',
  '',
  'RUNS  src/payments/processor.test.ts',
  '  PaymentProcessor',
  '    ✓ should process valid payment (123ms)',
  '    ✓ should reject invalid amount (15ms)',
  '    ○ skipped should handle network timeout',
  '',
  'RUNS  src/payments/validator.test.ts',
  '  PaymentValidator',
  '    ✓ should validate currency code (8ms)',
];

interface UseStageLogOptions {
  repoId: string;
  runId: string;
  jobKey: string;
  stepName: string;
  isRunning: boolean;
}

interface UseStageLogResult {
  stdout: string;
  stderr: string;
  loading: boolean;
  error: string | null;
  isPolling: boolean;
  totalSize: { stdout: number; stderr: number };
}

export function useStageLog({ repoId, runId, jobKey, stepName, isRunning }: UseStageLogOptions): UseStageLogResult {
  const [stdout, setStdout] = useState('');
  const [stderr, setStderr] = useState('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [isStreaming, setIsStreaming] = useState(false);
  const [totalSize, setTotalSize] = useState({ stdout: 0, stderr: 0 });

  const mockLineIndexRef = useRef(0);

  // Handler for log chunk events (real-time streaming)
  const handleLogChunk = useCallback((chunk: LogChunkEvent) => {
    if (chunk.stream === 'stdout') {
      setStdout((prev) => prev + chunk.content);
      setTotalSize((prev) => ({
        ...prev,
        stdout: prev.stdout + chunk.content.length,
      }));
    } else {
      setStderr((prev) => prev + chunk.content);
      setTotalSize((prev) => ({
        ...prev,
        stderr: prev.stderr + chunk.content.length,
      }));
    }
  }, []);

  // Subscribe to log chunk events when running (only in Tauri)
  useLogChunkEvents(repoId, runId, jobKey, stepName, handleLogChunk);

  const fetchLogs = useCallback(
    async (incremental: boolean = false) => {
      if (!isTauri) {
        // Use mock data for browser testing
        const mockLogs = mockStageLogs[stepName];
        if (mockLogs) {
          if (isRunning && stepName === 'test') {
            // Simulate growing output for running steps
            const lines = mockRunningOutput.slice(0, mockLineIndexRef.current);
            setStdout(lines.join('\n'));
            mockLineIndexRef.current = Math.min(mockLineIndexRef.current + 2, mockRunningOutput.length);
          } else {
            setStdout(mockLogs.stdout);
            setStderr(mockLogs.stderr);
          }
          setTotalSize({
            stdout: mockLogs.stdout.length,
            stderr: mockLogs.stderr.length,
          });
        }
        setLoading(false);
        return;
      }

      try {
        if (!incremental) {
          setLoading(true);
        }

        // Build artifact paths
        // Path format: ~/.airlock/artifacts/{repo_id}/{run_id}/logs/{job_key}/{step_name}/stdout.log
        const home = await homeDir();
        const basePath = `${home}/.airlock/artifacts/${repoId}/${runId}/logs/${jobKey}/${stepName}`;
        const stdoutPath = `${basePath}/stdout.log`;
        const stderrPath = `${basePath}/stderr.log`;

        // Fetch stdout (full read for initial load, events handle streaming)
        try {
          const stdoutResult = await readArtifact(stdoutPath, undefined, undefined);
          setStdout(stdoutResult.content);
          setTotalSize((prev) => ({
            ...prev,
            stdout: stdoutResult.total_size,
          }));
        } catch {
          // stdout might not exist yet
          if (!incremental) {
            setStdout('');
          }
        }

        // Fetch stderr (full read for initial load, events handle streaming)
        try {
          const stderrResult = await readArtifact(stderrPath, undefined, undefined);
          setStderr(stderrResult.content);
          setTotalSize((prev) => ({
            ...prev,
            stderr: stderrResult.total_size,
          }));
        } catch {
          // stderr might not exist yet
          if (!incremental) {
            setStderr('');
          }
        }

        setError(null);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setLoading(false);
      }
    },
    [repoId, runId, jobKey, stepName, isRunning]
  );

  // Initial fetch
  useEffect(() => {
    // Reset state when step changes
    setStdout('');
    setStderr('');
    setTotalSize({ stdout: 0, stderr: 0 });
    mockLineIndexRef.current = 3; // Start with a few lines for mock

    fetchLogs(false);
  }, [repoId, runId, jobKey, stepName, fetchLogs]);

  // Update streaming state based on isRunning
  useEffect(() => {
    setIsStreaming(isRunning && isTauri);
  }, [isRunning]);

  // Mock polling for browser mode only (events handle real-time in Tauri)
  useEffect(() => {
    // Only poll in browser mode for mock data simulation
    if (isTauri || !isRunning) {
      return;
    }

    // Simulate streaming for mock data in browser mode
    const pollInterval = setInterval(() => {
      fetchLogs(true);
    }, 1500);

    return () => {
      clearInterval(pollInterval);
    };
  }, [isRunning, fetchLogs]);

  return {
    stdout,
    stderr,
    loading,
    error,
    isPolling: isStreaming, // Keep the interface compatible
    totalSize,
  };
}
