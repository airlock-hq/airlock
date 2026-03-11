import { useState, useEffect } from 'react';
import { isTauri } from '../lib/tauri';
import { useDaemonHealth } from './use-daemon';

interface UpdateCheckResult {
  updateAvailable: boolean;
  latestVersion: string | null;
  currentVersion: string | null;
  releaseUrl: string | null;
}

const GITHUB_API_URL = 'https://api.github.com/repos/airlock-hq/airlock/releases/latest';
const CHECK_INTERVAL_MS = 4 * 60 * 60 * 1000; // 4 hours
const CACHE_KEY = 'airlock-update-check';

interface CachedResult {
  timestamp: number;
  result: UpdateCheckResult;
}

function compareSemver(a: string, b: string): number {
  const pa = a.split('.').map(Number);
  const pb = b.split('.').map(Number);
  for (let i = 0; i < 3; i++) {
    const diff = (pa[i] || 0) - (pb[i] || 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

function getCached(): UpdateCheckResult | null {
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw) return null;
    const cached: CachedResult = JSON.parse(raw);
    if (Date.now() - cached.timestamp < CHECK_INTERVAL_MS) {
      return cached.result;
    }
  } catch {
    // ignore
  }
  return null;
}

function setCache(result: UpdateCheckResult) {
  try {
    const cached: CachedResult = { timestamp: Date.now(), result };
    localStorage.setItem(CACHE_KEY, JSON.stringify(cached));
  } catch {
    // ignore
  }
}

async function fetchUpdateResult(version: string): Promise<UpdateCheckResult | null> {
  // In mock/dev mode, simulate an available update
  if (!isTauri) {
    return {
      updateAvailable: true,
      latestVersion: '0.2.0',
      currentVersion: version,
      releaseUrl: 'https://github.com/airlock-hq/airlock/releases/tag/v0.2.0',
    };
  }

  // Check localStorage cache first
  const cached = getCached();
  if (cached) {
    const updateAvailable = cached.latestVersion != null && compareSemver(cached.latestVersion, version) > 0;
    return { ...cached, currentVersion: version, updateAvailable };
  }

  try {
    const resp = await fetch(GITHUB_API_URL);
    if (!resp.ok) return null;
    const data = await resp.json();
    const tagName: string = data.tag_name ?? '';
    const latestVersion = tagName.replace(/^v/, '');
    const releaseUrl: string | null = data.html_url ?? null;

    const updateAvailable = compareSemver(latestVersion, version) > 0;
    const result = { updateAvailable, latestVersion, currentVersion: version, releaseUrl };
    setCache(result);
    return result;
  } catch {
    return null;
  }
}

const DEFAULT_RESULT: UpdateCheckResult = {
  updateAvailable: false,
  latestVersion: null,
  currentVersion: null,
  releaseUrl: null,
};

export function useUpdateCheck(): UpdateCheckResult {
  const { health } = useDaemonHealth();
  const currentVersion = health?.version ?? null;
  const [result, setResult] = useState<UpdateCheckResult>(DEFAULT_RESULT);

  useEffect(() => {
    if (!currentVersion) return;
    const version = currentVersion;
    let cancelled = false;

    function check() {
      fetchUpdateResult(version).then((r) => {
        if (!cancelled && r) {
          setResult(r);
        }
      });
    }

    check();
    const interval = setInterval(check, CHECK_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [currentVersion]);

  return result;
}
