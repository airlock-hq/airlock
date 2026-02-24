import type { Meta, StoryObj } from '@storybook/react';
import { useState, useMemo } from 'react';
import { Button } from '../react/button';
import { Input } from '../react/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../react/select';
import { StatusDot } from '../react/status-dot';
import { Atmosphere } from '../react/atmosphere';
import { GitBranch, RefreshCw, Search, Loader2 } from 'lucide-react';

const meta: Meta = {
  title: 'Pages/Runs',
  parameters: {
    layout: 'fullscreen',
  },
};

export default meta;
type Story = StoryObj;

/* ---------- Mock Data ---------- */

interface MockRun {
  id: string;
  repoName: string;
  branch: string;
  status: 'running' | 'awaiting_approval' | 'completed' | 'failed';
  time: string;
  error?: string;
}

const mockRuns: MockRun[] = [
  { id: 'run-1', repoName: 'acme/backend', branch: 'feat/auth-flow', status: 'running', time: '2m ago' },
  { id: 'run-2', repoName: 'acme/backend', branch: 'fix/login-bug', status: 'awaiting_approval', time: '15m ago' },
  { id: 'run-3', repoName: 'acme/frontend', branch: 'feat/dark-mode', status: 'completed', time: '1 hour ago' },
  { id: 'run-4', repoName: 'acme/backend', branch: 'refactor/db-layer', status: 'completed', time: '3 hours ago' },
  {
    id: 'run-5',
    repoName: 'acme/infra',
    branch: 'feat/k8s-deploy',
    status: 'failed',
    time: '5 hours ago',
    error: 'Stage "test" failed: assertion error in test_deploy_config',
  },
  { id: 'run-6', repoName: 'acme/frontend', branch: 'fix/responsive-nav', status: 'completed', time: 'yesterday' },
  { id: 'run-7', repoName: 'acme/backend', branch: 'feat/webhooks', status: 'awaiting_approval', time: 'yesterday' },
];

const errorRuns: MockRun[] = [
  {
    id: 'run-e1',
    repoName: 'acme/backend',
    branch: 'feat/broken-migration',
    status: 'failed',
    time: '10m ago',
    error: 'Stage "describe" failed: LLM returned empty response after 3 retries',
  },
  {
    id: 'run-e2',
    repoName: 'acme/frontend',
    branch: 'fix/build-error',
    status: 'failed',
    time: '45m ago',
    error: 'Stage "test" failed: npm ERR! Cannot find module \'react-dom/client\'',
  },
  {
    id: 'run-e3',
    repoName: 'acme/infra',
    branch: 'feat/terraform-update',
    status: 'failed',
    time: '2 hours ago',
    error: 'Stage "push" failed: remote rejected — branch protection rules require review',
  },
];

/* ---------- Helpers ---------- */

function RunStatusDot({ status }: { status: MockRun['status'] }) {
  const props: Record<string, { variant: 'warning' | 'signal' | 'success' | 'danger'; pulse?: boolean }> = {
    running: { variant: 'warning' },
    awaiting_approval: { variant: 'signal', pulse: true },
    completed: { variant: 'success' },
    failed: { variant: 'danger' },
  };
  const { variant, pulse } = props[status] || { variant: 'muted' as const };
  return <StatusDot variant={variant} size="md" pulse={pulse} />;
}

function statusLabel(status: MockRun['status']): string {
  switch (status) {
    case 'running':
      return 'Running';
    case 'awaiting_approval':
      return 'Awaiting';
    case 'completed':
      return 'Completed';
    case 'failed':
      return 'Failed';
  }
}

/* ---------- Components ---------- */

function RunsPage({ runs, loading = false }: { runs: MockRun[]; loading?: boolean }) {
  const [statusFilter, setStatusFilter] = useState('all');
  const [searchQuery, setSearchQuery] = useState('');

  const filteredRuns = useMemo(() => {
    let filtered = runs;
    if (statusFilter !== 'all') filtered = filtered.filter((r) => r.status === statusFilter);
    if (searchQuery.trim()) {
      const q = searchQuery.toLowerCase();
      filtered = filtered.filter((r) => r.branch.toLowerCase().includes(q) || r.repoName.toLowerCase().includes(q));
    }
    return filtered;
  }, [runs, statusFilter, searchQuery]);

  return (
    <div className="bg-background relative flex min-h-screen flex-col">
      <Atmosphere />

      <div className="relative z-10 flex flex-1 flex-col px-8 py-8">
        {/* Header */}
        <div className="mb-8 flex items-end justify-end">
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
            <Button variant="ghost" size="icon">
              <RefreshCw className="h-4 w-4" />
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

          {loading ? (
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
                <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">
                  NO RUNS YET
                </span>
                <p className="text-small text-foreground-muted max-w-sm text-center">
                  Run <code className="bg-surface text-micro rounded px-1.5 py-0.5 font-mono">airlock init</code> in a
                  git repository, then push your changes.
                </p>
              </div>
            )
          ) : (
            filteredRuns.map((run) => (
              <div
                key={run.id}
                className="border-border-subtle/50 hover:bg-surface/30 grid grid-cols-[2rem_1fr_1fr_8rem_6rem] items-center gap-6 border-b px-6 py-4 transition-colors last:border-b-0"
              >
                <RunStatusDot status={run.status} />
                <div className="min-w-0">
                  <span className="text-foreground font-medium">{run.branch}</span>
                  {run.error && <p className="text-micro text-danger mt-1 truncate">{run.error}</p>}
                </div>
                <span className="text-small text-foreground-muted">{run.repoName}</span>
                <span className="text-micro text-foreground-muted font-mono">{run.time}</span>
                <div className="flex items-center justify-end">
                  <div className="text-micro text-foreground-muted/60 font-mono">{statusLabel(run.status)}</div>
                </div>
              </div>
            ))
          )}
        </div>
      </div>

      <style>{`
        @keyframes pulse {
          0%, 100% { opacity: 1; }
          50% { opacity: 0.4; }
        }
      `}</style>
    </div>
  );
}

/* ---------- Stories ---------- */

export const Populated: Story = {
  render: () => <RunsPage runs={mockRuns} />,
};

export const Empty: Story = {
  render: () => <RunsPage runs={[]} />,
};

export const Loading: Story = {
  render: () => <RunsPage runs={[]} loading />,
};

export const Filtered: Story = {
  render: () => <RunsPage runs={mockRuns.filter((r) => r.status === 'awaiting_approval')} />,
};

export const WithErrors: Story = {
  render: () => <RunsPage runs={errorRuns} />,
};
