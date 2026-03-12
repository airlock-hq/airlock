import type { ReactNode } from 'react';
import { createElement } from 'react';
import type { ArtifactInfo } from '@/hooks/use-daemon';

/**
 * Utility functions for StageLogViewer components.
 */

export type TabItem = {
  type: 'artifact' | 'log';
  key: string;
  label: string;
  artifact?: ArtifactInfo;
  logType?: 'stdout' | 'stderr';
};

/**
 * Get sort order for tab items.
 * Priority: md files (0) → json files (1) → other artifacts (2) → stdout (3) → stderr (4)
 */
export function getTabSortOrder(item: TabItem): number {
  if (item.type === 'artifact' && item.artifact) {
    const ext = item.artifact.name.split('.').pop()?.toLowerCase();
    if (ext === 'md') return 0; // Markdown first
    if (ext === 'json') return 1; // JSON second
    return 2; // Other artifacts
  }
  if (item.logType === 'stdout') return 3;
  if (item.logType === 'stderr') return 4;
  return 5;
}

/**
 * Build sorted tab items from artifacts and log availability.
 */
export function buildTabItems(artifacts: ArtifactInfo[], hasStdout: boolean, hasStderr: boolean): TabItem[] {
  const items: TabItem[] = [];

  // Add artifact tabs
  for (const artifact of artifacts) {
    items.push({
      type: 'artifact',
      key: `artifact-${artifact.path}`,
      label: artifact.name,
      artifact,
    });
  }

  // Add log tabs
  if (hasStdout) {
    items.push({
      type: 'log',
      key: 'stdout',
      label: 'stdout',
      logType: 'stdout',
    });
  }

  if (hasStderr) {
    items.push({
      type: 'log',
      key: 'stderr',
      label: 'stderr',
      logType: 'stderr',
    });
  }

  // Sort by priority
  return items.sort((a, b) => getTabSortOrder(a) - getTabSortOrder(b));
}

/**
 * Get file extension from artifact name.
 */
export function getFileExtension(filename: string): string {
  return filename.split('.').pop()?.toLowerCase() ?? '';
}

export function formatStageName(stage: string): string {
  const abbreviations = new Set(['pr', 'api', 'db', 'ui', 'id', 'url', 'sql', 'css', 'html', 'json', 'xml']);

  return stage
    .split(/[-_]/)
    .map((word) => {
      const lower = word.toLowerCase();
      if (abbreviations.has(lower)) {
        return word.toUpperCase();
      }
      return word.charAt(0).toUpperCase() + word.slice(1).toLowerCase();
    })
    .join(' ');
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  return `${(ms / 60000).toFixed(1)}m`;
}

export function escapeRegex(string: string): string {
  return string.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** Highlight matching search terms within a line, returning ReactNode content. */
export function highlightLine(line: string, searchQuery: string, highlightClass = 'bg-warning/20'): ReactNode {
  if (!searchQuery) return line;
  const regex = new RegExp(`(${escapeRegex(searchQuery)})`, 'gi');
  return line
    .split(regex)
    .map((part, i) => (regex.test(part) ? createElement('mark', { key: i, className: highlightClass }, part) : part));
}

export function getArtifactTypeName(artifactType: string): string {
  switch (artifactType) {
    case 'description':
      return 'PR Description';
    case 'analysis':
      return 'Code Analysis';
    case 'test_results':
    case 'test':
      return 'Test Results';
    case 'coverage':
      return 'Coverage Report';
    default:
      return artifactType.charAt(0).toUpperCase() + artifactType.slice(1);
  }
}
