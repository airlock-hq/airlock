import type { Meta, StoryObj } from '@storybook/react';
import { Button } from '../react/button';
import { Badge } from '../react/badge';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../react/card';
import { StatusDot } from '../react/status-dot';
import { Atmosphere } from '../react/atmosphere';
import { GitBranch, RefreshCw, ExternalLink, Clock, FolderOpen } from 'lucide-react';

const meta: Meta = {
  title: 'Pages/Repositories',
  parameters: {
    layout: 'fullscreen',
  },
};

export default meta;
type Story = StoryObj;

/* ---------- Mock Data ---------- */

interface MockRepo {
  id: string;
  name: string;
  upstreamUrl: string;
  workingPath: string;
  pendingRuns: number;
  lastSync: string | null;
}

const mockRepos: MockRepo[] = [
  {
    id: 'repo-1',
    name: 'acme/backend',
    upstreamUrl: 'git@github.com:acme/backend.git',
    workingPath: '/Users/dev/projects/acme/backend',
    pendingRuns: 2,
    lastSync: '5m ago',
  },
  {
    id: 'repo-2',
    name: 'acme/frontend',
    upstreamUrl: 'git@github.com:acme/frontend.git',
    workingPath: '/Users/dev/projects/acme/frontend',
    pendingRuns: 0,
    lastSync: '1h ago',
  },
  {
    id: 'repo-3',
    name: 'acme/infra',
    upstreamUrl: 'https://github.com/acme/infra.git',
    workingPath: '/Users/dev/projects/acme/infra',
    pendingRuns: 0,
    lastSync: null,
  },
];

/* ---------- Components ---------- */

function RepoCard({ repo }: { repo: MockRepo }) {
  return (
    <Card className="border-border-subtle bg-background/60 hover:border-signal/40 transition-colors">
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="flex items-center gap-3">
            <GitBranch className="text-signal h-5 w-5" />
            <CardTitle className="text-body">{repo.name}</CardTitle>
          </div>
          <div className="flex items-center gap-2">
            {repo.pendingRuns > 0 && <Badge variant="warning">{repo.pendingRuns} pending</Badge>}
            <Badge variant={repo.lastSync ? 'success' : 'secondary'}>{repo.lastSync ? 'Synced' : 'Never synced'}</Badge>
          </div>
        </div>
        <CardDescription className="text-micro mt-2 flex items-center gap-1.5 font-mono">
          <ExternalLink className="h-3 w-3" />
          {repo.upstreamUrl}
        </CardDescription>
      </CardHeader>
      <CardContent>
        <div className="flex items-center justify-between">
          <div className="text-small text-foreground-muted flex items-center gap-6">
            <div className="flex items-center gap-1.5">
              <FolderOpen className="h-3.5 w-3.5" />
              <span className="text-micro max-w-[240px] truncate font-mono">{repo.workingPath}</span>
            </div>
            {repo.lastSync && (
              <div className="flex items-center gap-1.5">
                <Clock className="h-3.5 w-3.5" />
                <span className="text-micro font-mono">{repo.lastSync}</span>
              </div>
            )}
          </div>
          <div className="flex items-center gap-2">
            <Button variant="outline" size="sm" className="border-border-subtle text-micro h-8">
              <RefreshCw className="mr-1.5 h-3.5 w-3.5" />
              Sync
            </Button>
            <Button size="sm" className="text-micro h-8">
              View Runs
            </Button>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

function ReposPage({ repos, error }: { repos: MockRepo[]; error?: string }) {
  return (
    <div className="bg-background relative min-h-screen">
      <Atmosphere />

      <div className="relative z-10 px-8 py-8">
        {/* Header */}
        <div className="mb-8 flex items-end justify-between">
          <div>
            <h1 className="text-h1 text-foreground font-bold tracking-tight">Repositories</h1>
            <p className="text-small text-foreground-muted mt-1">Manage your Airlock-enrolled repositories</p>
          </div>
          <Button variant="outline" className="border-border-subtle bg-background/60">
            <RefreshCw className="mr-2 h-4 w-4" />
            Refresh
          </Button>
        </div>

        {/* Error */}
        {error && (
          <div className="border-danger/30 bg-danger/5 mb-6 flex items-center gap-3 rounded-lg border px-4 py-3">
            <StatusDot variant="danger" size="md" />
            <span className="text-small text-danger">{error}</span>
          </div>
        )}

        {/* Content */}
        {repos.length === 0 ? (
          <div className="border-border-subtle bg-background/60 flex flex-col items-center gap-6 rounded-lg border py-20">
            <GitBranch className="text-foreground-muted/30 h-12 w-12" />
            <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">
              NO REPOSITORIES ENROLLED
            </span>
            <p className="text-small text-foreground-muted max-w-md text-center">
              Airlock intercepts your git pushes and transforms them into clean, reviewable changes.
            </p>
            <div className="bg-surface/60 rounded-md px-6 py-4">
              <p className="text-micro text-foreground-muted mb-2 font-mono tracking-widest uppercase">QUICK START</p>
              <code className="text-micro text-foreground block font-mono">
                cd your-project
                <br />
                airlock init
              </code>
            </div>
          </div>
        ) : (
          <div className="grid gap-4">
            {repos.map((repo) => (
              <RepoCard key={repo.id} repo={repo} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

/* ---------- Stories ---------- */

export const Populated: Story = {
  render: () => <ReposPage repos={mockRepos} />,
};

export const Empty: Story = {
  render: () => <ReposPage repos={[]} />,
};

export const WithError: Story = {
  render: () => (
    <ReposPage
      repos={mockRepos.slice(0, 1)}
      error="Failed to connect to daemon: connection refused. Is airlockd running?"
    />
  ),
};
