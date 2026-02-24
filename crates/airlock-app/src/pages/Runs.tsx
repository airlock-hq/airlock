import { useState, useMemo } from 'react';
import { Button, StatusDot } from '@airlock-hq/design-system/react';
import { Input } from '@airlock-hq/design-system/react';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@airlock-hq/design-system/react';
import { useAllRuns, type RunInfo } from '@/hooks/use-daemon';
import { GitBranch, RefreshCw, Loader2, Search } from 'lucide-react';
import { Link, useSearchParams } from 'react-router-dom';
import { cn } from '@/lib/utils';

function formatTimeAgo(timestamp: number): string {
  const now = Date.now() / 1000;
  const diff = now - timestamp;

  if (diff < 60) return 'just now';
  if (diff < 3600) {
    const mins = Math.floor(diff / 60);
    return `${mins} minute${mins !== 1 ? 's' : ''} ago`;
  }
  if (diff < 86400) {
    const hours = Math.floor(diff / 3600);
    return `${hours} hour${hours !== 1 ? 's' : ''} ago`;
  }
  if (diff < 604800) {
    const days = Math.floor(diff / 86400);
    if (days === 1) return 'yesterday';
    return `${days} days ago`;
  }
  const weeks = Math.floor(diff / 604800);
  return `${weeks} week${weeks !== 1 ? 's' : ''} ago`;
}

interface RunWithRepo extends RunInfo {
  repo_name: string;
}

function RunStatusDot({ status }: { status: string }) {
  const props: Record<string, { variant: 'success' | 'danger' | 'warning' | 'signal' | 'muted'; pulse?: boolean }> = {
    running: { variant: 'warning' },
    awaiting_approval: { variant: 'signal', pulse: true },
    completed: { variant: 'success' },
    failed: { variant: 'danger' },
    superseded: { variant: 'muted' },
  };
  const { variant, pulse } = props[status] || { variant: 'muted' as const };
  return <StatusDot variant={variant} size="md" pulse={pulse} />;
}

function statusLabel(status: string): string {
  switch (status) {
    case 'running':
      return 'Running';
    case 'awaiting_approval':
      return 'Awaiting';
    case 'completed':
      return 'Completed';
    case 'failed':
      return 'Failed';
    case 'superseded':
      return 'Superseded';
    default:
      return status;
  }
}

function RunRow({ run }: { run: RunWithRepo }) {
  const derivedStatus = getDerivedStatus(run.status);

  return (
    <div className="border-border-subtle/50 hover:bg-surface/30 grid grid-cols-[2rem_1fr_1fr_8rem_6rem] items-center gap-6 border-b px-6 py-4 transition-colors last:border-b-0">
      <RunStatusDot status={derivedStatus} />
      <div className="min-w-0">
        <Link
          to={`/repos/${run.repo_id}/runs/${run.id}`}
          className="text-foreground hover:text-signal font-medium hover:underline"
        >
          {run.branch || `Run #${run.id.slice(-8)}`}
        </Link>
        {run.error && <p className="text-micro text-danger mt-1 truncate">{run.error}</p>}
      </div>
      <span className="text-small text-foreground-muted">{run.repo_name}</span>
      <span className="text-micro text-foreground-muted font-mono">{formatTimeAgo(run.created_at)}</span>
      <div className="flex items-center justify-end">
        <div className="text-micro text-foreground-muted/60 font-mono">{statusLabel(derivedStatus)}</div>
      </div>
    </div>
  );
}

/**
 * Derives display status from run status string.
 * Maps both legacy (pending_review) and new (awaiting_approval) statuses.
 */
function getDerivedStatus(status: string): 'running' | 'awaiting_approval' | 'completed' | 'failed' | 'superseded' {
  switch (status) {
    case 'running':
      return 'running';
    case 'pending_review':
    case 'awaiting_approval':
      return 'awaiting_approval';
    case 'failed':
      return 'failed';
    case 'superseded':
      return 'superseded';
    case 'forwarded':
    case 'completed':
    default:
      return 'completed';
  }
}

export function Runs() {
  const [searchParams, setSearchParams] = useSearchParams();
  const { runs, loading: runsLoading, refresh: refreshRuns } = useAllRuns(50);
  const searchQuery = searchParams.get('filter') ?? '';
  const [statusFilter, setStatusFilter] = useState<string>('all');

  const setSearchQuery = (query: string) => {
    if (query) {
      setSearchParams({ filter: query }, { replace: true });
    } else {
      setSearchParams({}, { replace: true });
    }
  };

  // Filter and search runs
  const filteredRuns = useMemo(() => {
    let filtered = runs;

    // Status filter
    if (statusFilter !== 'all') {
      filtered = filtered.filter((r) => {
        const derived = getDerivedStatus(r.status);
        return derived === statusFilter;
      });
    }

    // Search filter
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase();
      filtered = filtered.filter(
        (r) =>
          r.branch?.toLowerCase().includes(query) ||
          r.repo_name.toLowerCase().includes(query) ||
          r.id.toLowerCase().includes(query)
      );
    }

    return filtered;
  }, [runs, statusFilter, searchQuery]);

  return (
    <div className="flex h-full flex-col gap-6">
      {/* Header */}
      <div className="flex items-end justify-end">
        <div className="flex items-center gap-3">
          <Select value={statusFilter} onValueChange={setStatusFilter}>
            <SelectTrigger className="border-border-subtle bg-background/60 w-[140px]">
              <SelectValue placeholder="Status" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All</SelectItem>
              <SelectItem value="awaiting_approval">Awaiting</SelectItem>
              <SelectItem value="running">Running</SelectItem>
              <SelectItem value="completed">Completed</SelectItem>
              <SelectItem value="failed">Failed</SelectItem>
            </SelectContent>
          </Select>
          <div className="relative w-72">
            <Search className="text-foreground-muted absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2" />
            <Input
              placeholder="Filter runs..."
              value={searchQuery}
              onChange={(e: { target: { value: string } }) => setSearchQuery(e.target.value)}
              className="border-border-subtle bg-background/60 pl-9"
            />
          </div>
          <Button variant="ghost" size="icon" onClick={refreshRuns} disabled={runsLoading}>
            <RefreshCw className={cn('h-4 w-4', runsLoading && 'animate-spin')} />
          </Button>
        </div>
      </div>

      {/* Table */}
      <div className="border-border-subtle bg-background/60 rounded-lg border">
        {/* Column Headers */}
        <div className="border-border-subtle grid grid-cols-[2rem_1fr_1fr_8rem_6rem] items-center gap-6 border-b px-6 py-3">
          <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase"></span>
          <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">Branch</span>
          <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">Repository</span>
          <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">Time</span>
          <span />
        </div>

        {runsLoading ? (
          <div className="flex items-center justify-center py-20">
            <Loader2 className="text-foreground-muted h-5 w-5 animate-spin" />
          </div>
        ) : filteredRuns.length === 0 ? (
          searchQuery || statusFilter !== 'all' ? (
            <div className="flex flex-col items-center gap-4 py-20">
              <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">
                NO MATCHING RUNS
              </span>
              <p className="text-small text-foreground-muted">Try adjusting your search or filters.</p>
              <Button
                variant="link"
                onClick={() => {
                  setSearchQuery('');
                  setStatusFilter('all');
                }}
              >
                Clear filters
              </Button>
            </div>
          ) : (
            <div className="flex flex-col items-center gap-4 py-20">
              <GitBranch className="text-foreground-muted/30 h-10 w-10" />
              <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">NO RUNS YET</span>
              <p className="text-small text-foreground-muted max-w-sm text-center">
                Run <code className="bg-surface text-micro rounded px-1.5 py-0.5 font-mono">airlock init</code> in a git
                repository, then push your changes.
              </p>
            </div>
          )
        ) : (
          filteredRuns.map((run) => <RunRow key={`${run.repo_id}_${run.id}`} run={run} />)
        )}
      </div>
    </div>
  );
}
