import type { Meta, StoryObj } from '@storybook/react';
import { useState } from 'react';
import { Button } from '../react/button';
import { StatusDot } from '../react/status-dot';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../react/tabs';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../react/tooltip';
import { Atmosphere } from '../react/atmosphere';
import { CheckCircle2, RefreshCw, GitBranch, GitCommit, FileText, FileDiff, Layers, Activity } from 'lucide-react';

const meta: Meta = {
  title: 'Pages/Run Detail',
  parameters: {
    layout: 'fullscreen',
  },
};

export default meta;
type Story = StoryObj;

/* ---------- Mock Data ---------- */

interface MockStage {
  name: string;
  status: 'passed' | 'failed' | 'running' | 'skipped' | 'pending' | 'awaiting_approval';
  duration?: string;
  error?: string;
}

const completedStages: MockStage[] = [
  { name: 'describe', status: 'passed', duration: '4.2s' },
  { name: 'test', status: 'passed', duration: '18.7s' },
  { name: 'push', status: 'passed', duration: '1.8s' },
  { name: 'create-pr', status: 'passed', duration: '3.5s' },
];

const runningStages: MockStage[] = [
  { name: 'describe', status: 'passed', duration: '4.2s' },
  { name: 'test', status: 'passed', duration: '18.7s' },
  { name: 'push', status: 'running' },
  { name: 'create-pr', status: 'pending' },
];

const awaitingStages: MockStage[] = [
  { name: 'describe', status: 'passed', duration: '4.2s' },
  { name: 'test', status: 'passed', duration: '18.7s' },
  { name: 'push', status: 'awaiting_approval' },
  { name: 'create-pr', status: 'pending' },
];

const failedStages: MockStage[] = [
  { name: 'describe', status: 'passed', duration: '4.2s' },
  { name: 'test', status: 'failed', duration: '8.7s', error: 'Exit code 1' },
  { name: 'push', status: 'skipped' },
  { name: 'create-pr', status: 'skipped' },
];

const mockLogLines = [
  '$ running pipeline stage: push',
  '> Loading diff from base..head (3 files changed)',
  '> Analyzing changes with LLM...',
  '> File: src/auth/handler.rs',
  '  + Added OAuth2 token refresh logic',
  '  + Updated error handling for expired tokens',
  '> File: src/auth/mod.rs',
  '  + Added public re-export for new handler',
  '> File: tests/auth_test.rs',
  '  + Added integration tests for token refresh',
  '> Review complete: 3 files analyzed, 0 issues found',
  '> Generating PR description...',
  '> Stage completed successfully in 12.1s',
];

/* ---------- Helpers ---------- */

function getStageStatusDot(status: MockStage['status']) {
  switch (status) {
    case 'passed':
      return <StatusDot variant="success" size="md" />;
    case 'failed':
      return <StatusDot variant="danger" size="md" />;
    case 'running':
      return <StatusDot variant="warning" size="md" pulse />;
    case 'awaiting_approval':
      return <StatusDot variant="signal" size="md" pulse />;
    case 'skipped':
    case 'pending':
    default:
      return <StatusDot variant="muted" size="md" />;
  }
}

function formatStageName(stage: string): string {
  const abbr = new Set(['pr', 'api', 'db', 'ui']);
  return stage
    .split(/[-_]/)
    .map((w) => (abbr.has(w.toLowerCase()) ? w.toUpperCase() : w.charAt(0).toUpperCase() + w.slice(1).toLowerCase()))
    .join(' ');
}

function getRunStatusDotProps(status: string): {
  variant: 'success' | 'danger' | 'warning' | 'signal' | 'muted';
  pulse?: boolean;
} {
  switch (status) {
    case 'running':
      return { variant: 'warning' };
    case 'awaiting_approval':
      return { variant: 'signal', pulse: true };
    case 'completed':
      return { variant: 'success' };
    case 'failed':
      return { variant: 'danger' };
    default:
      return { variant: 'muted' };
  }
}

/* ---------- Components ---------- */

function StagesSidebar({
  stages,
  selectedStage,
  onSelectStage,
}: {
  stages: MockStage[];
  selectedStage: string;
  onSelectStage: (n: string) => void;
}) {
  return (
    <div className="border-border-subtle flex h-full w-56 flex-col border-r">
      <div className="border-border-subtle border-b px-4 py-3">
        <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">Stages</span>
      </div>
      <div className="flex-1 overflow-y-auto">
        {stages.map((stage) => (
          <div
            key={stage.name}
            className={`cursor-pointer px-4 py-3 transition-colors ${
              selectedStage === stage.name
                ? 'border-l-signal bg-surface/30 border-l-2'
                : 'hover:bg-surface/20 border-l-2 border-l-transparent'
            }`}
            onClick={() => onSelectStage(stage.name)}
          >
            <div className="flex items-center gap-2.5">
              {getStageStatusDot(stage.status)}
              <div className="min-w-0 flex-1">
                <div className="flex items-center justify-between gap-2">
                  <span className="text-small truncate">{formatStageName(stage.name)}</span>
                  {stage.duration && (
                    <span className="text-micro text-foreground-muted shrink-0 font-mono">{stage.duration}</span>
                  )}
                </div>
                {stage.error && <p className="text-micro text-danger mt-0.5 truncate">{stage.error}</p>}
              </div>
            </div>
            {stage.status === 'awaiting_approval' && (
              <div className="mt-2">
                <Button size="sm" className="text-micro h-7 w-full">
                  <CheckCircle2 className="mr-1 h-3 w-3" /> Approve
                </Button>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function OverviewContent({
  status,
  branch,
  headSha,
  baseSha,
  error,
}: {
  status: string;
  branch: string;
  headSha: string;
  baseSha: string;
  error?: string;
}) {
  const statusDotColor =
    status === 'completed' ? 'success' : status === 'failed' ? 'danger' : status === 'running' ? 'warning' : 'signal';
  const statusLabel = status === 'awaiting_approval' ? 'AWAITING APPROVAL' : status.toUpperCase();

  return (
    <div className="h-full overflow-y-auto p-8">
      {/* Summary telemetry grid */}
      <div className="mb-12">
        <h2 className="text-micro text-foreground-muted mb-4 font-mono tracking-widest uppercase">Push Request</h2>
        <div className="grid grid-cols-2 gap-x-12 gap-y-4">
          {[
            { label: 'BRANCH', value: branch },
            { label: 'STATUS', value: statusLabel, dot: statusDotColor },
            { label: 'BASE', value: baseSha },
            { label: 'HEAD', value: headSha },
          ].map((item) => (
            <div key={item.label} className="flex items-baseline gap-3">
              <span className="text-micro text-foreground-muted w-20 font-mono tracking-widest uppercase">
                {item.label}
              </span>
              <span className="text-micro text-foreground-muted">&mdash;</span>
              <span className="text-micro text-foreground inline-flex items-center gap-2 font-mono font-semibold tracking-widest uppercase">
                {'dot' in item && item.dot && (
                  <StatusDot
                    variant={item.dot as 'success' | 'danger' | 'warning' | 'signal' | 'muted'}
                    className="inline-block"
                  />
                )}
                {item.value}
              </span>
            </div>
          ))}
        </div>
        {error && (
          <div className="border-danger/20 bg-danger/5 mt-6 rounded-md border px-4 py-3">
            <p className="text-small text-danger">{error}</p>
          </div>
        )}
      </div>

      {/* Content artifact */}
      <div className="border-border-subtle rounded-md border p-6">
        <h3 className="text-body mb-4 flex items-center gap-2 font-medium">
          <FileText className="text-foreground-muted h-4 w-4" />
          Push Request Description
        </h3>
        <div className="text-small text-foreground-muted space-y-4">
          <p>This push request adds OAuth2 token refresh logic to the authentication handler.</p>
          <h4 className="text-foreground font-semibold">Changes</h4>
          <ul className="list-inside list-disc space-y-1">
            <li>Added automatic token refresh on 401 responses</li>
            <li>Updated error handling to distinguish between expired and invalid tokens</li>
            <li>Added integration tests covering the refresh flow</li>
          </ul>
          <h4 className="text-foreground font-semibold">Files Changed</h4>
          <div className="border-terminal-border bg-terminal text-micro text-terminal-foreground rounded border p-4 font-mono leading-relaxed">
            src/auth/handler.rs &nbsp;| 42 +++++++++++---
            <br />
            src/auth/mod.rs &nbsp;&nbsp;&nbsp;&nbsp;| &nbsp;3 +<br />
            tests/auth_test.rs &nbsp;| 28 +++++++
            <br />3 files changed, 65 insertions(+), 8 deletions(-)
          </div>
        </div>
      </div>
    </div>
  );
}

function ActivityContent({ stages }: { stages: MockStage[] }) {
  const [selectedStage, setSelectedStage] = useState(stages[0].name);
  return (
    <div className="flex h-full min-h-0">
      <StagesSidebar stages={stages} selectedStage={selectedStage} onSelectStage={setSelectedStage} />
      <div className="flex flex-1 flex-col">
        <div className="border-border-subtle flex items-center justify-between border-b px-4 py-2.5">
          <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">
            {formatStageName(selectedStage)}
          </span>
          <span className="text-micro text-foreground-muted font-mono">
            {stages.find((s) => s.name === selectedStage)?.duration || '—'}
          </span>
        </div>
        <div className="border-terminal-border bg-terminal flex-1 overflow-auto border p-4">
          <pre className="text-micro text-terminal-foreground font-mono leading-loose">
            {mockLogLines.map((line, i) => (
              <div
                key={i}
                className={
                  line.startsWith('>') || line.startsWith('$')
                    ? 'text-terminal-foreground'
                    : 'text-terminal-foreground/50'
                }
              >
                {line}
              </div>
            ))}
          </pre>
        </div>
      </div>
    </div>
  );
}

function RunDetailPage({
  repo,
  branch,
  status,
  stages,
  headSha = 'a1b2c3d',
  baseSha = 'e4f5g6h',
  error,
  defaultTab = 'overview',
}: {
  repo: string;
  branch: string;
  status: string;
  stages: MockStage[];
  headSha?: string;
  baseSha?: string;
  error?: string;
  defaultTab?: string;
}) {
  const [activeTab, setActiveTab] = useState(defaultTab);
  const patchCount = 3;

  return (
    <TooltipProvider>
      <div className="bg-background relative flex h-screen flex-col">
        <Atmosphere />

        <div className="relative z-10 flex flex-1 flex-col px-8 py-6">
          {/* Header */}
          <div className="mb-6 flex items-start justify-between">
            <div>
              <div className="flex items-center gap-3">
                <h1 className="text-h2 text-foreground font-bold tracking-tight">{branch}</h1>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <StatusDot {...getRunStatusDotProps(status)} />
                  </TooltipTrigger>
                  <TooltipContent>
                    {status === 'awaiting_approval'
                      ? 'Awaiting Approval'
                      : status.charAt(0).toUpperCase() + status.slice(1)}
                  </TooltipContent>
                </Tooltip>
              </div>
              <div className="text-micro text-foreground-muted mt-2 flex items-center gap-3 font-mono">
                <div className="flex items-center gap-1.5">
                  <GitBranch className="h-3 w-3" />
                  <span>{repo}</span>
                </div>
                <span>&middot;</span>
                <div className="flex items-center gap-1.5">
                  <GitCommit className="h-3 w-3" />
                  <span className="uppercase">{headSha}</span>
                </div>
                <span>&middot;</span>
                <span>15m ago</span>
              </div>
            </div>
            <div className="flex items-center gap-2">
              {status !== 'running' && (
                <Button variant="outline" size="sm" className="border-border-subtle text-micro">
                  <RefreshCw className="mr-1.5 h-3.5 w-3.5" /> Reprocess
                </Button>
              )}
              <Button variant="ghost" size="sm">
                <RefreshCw className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>

          {/* Error display */}
          {error && (
            <div className="border-danger/20 bg-danger/5 mb-4 rounded-md border px-4 py-2.5">
              <p className="text-small text-danger">{error}</p>
            </div>
          )}

          {/* Main content */}
          <div className="border-border-subtle bg-background/60 flex min-h-0 flex-1 flex-col rounded-lg border">
            <Tabs value={activeTab} onValueChange={setActiveTab} className="flex min-h-0 flex-1 flex-col">
              <TabsList variant="line" className="w-full justify-start px-6 pt-2">
                <TabsTrigger variant="line" value="overview">
                  <FileText className="mr-2 h-4 w-4" /> Overview
                </TabsTrigger>
                <TabsTrigger variant="line" value="changes">
                  <FileDiff className="mr-2 h-4 w-4" /> Changes
                </TabsTrigger>
                <TabsTrigger variant="line" value="patches">
                  <Layers className="mr-2 h-4 w-4" /> Patches
                  {patchCount > 0 && (
                    <span className="bg-signal/20 text-micro ml-2 rounded-full px-2 py-0.5">{patchCount}</span>
                  )}
                </TabsTrigger>
                <TabsTrigger variant="line" value="activity">
                  <Activity className="mr-2 h-4 w-4" /> Activity
                </TabsTrigger>
              </TabsList>

              <TabsContent value="overview" className="mt-0 min-h-0 flex-1">
                <OverviewContent status={status} branch={branch} headSha={headSha} baseSha={baseSha} error={error} />
              </TabsContent>

              <TabsContent value="changes" className="mt-0 min-h-0 flex-1">
                <div className="flex h-full flex-col items-center justify-center gap-4">
                  <FileDiff className="text-foreground-muted/30 h-10 w-10" />
                  <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">
                    DIFF VIEWER
                  </span>
                  <p className="text-small text-foreground-muted">3 files changed, +65 -8</p>
                </div>
              </TabsContent>

              <TabsContent value="patches" className="mt-0 min-h-0 flex-1">
                <div className="p-6">
                  <div className="mb-6 flex items-center gap-3">
                    <Button variant="outline" size="sm" className="border-border-subtle text-micro h-7">
                      Select All
                    </Button>
                    <Button variant="outline" size="sm" className="border-border-subtle text-micro h-7">
                      Select None
                    </Button>
                    <Button size="sm" className="text-micro h-7">
                      Apply Selected (0)
                    </Button>
                  </div>
                  <div className="space-y-3">
                    {['Add OAuth2 token refresh', 'Update error handling', 'Add integration tests'].map((title, i) => (
                      <div
                        key={i}
                        className="border-border-subtle bg-background/40 flex items-center gap-4 rounded-lg border px-4 py-3"
                      >
                        <input type="checkbox" className="border-border h-4 w-4 rounded" />
                        <div className="flex-1">
                          <p className="text-small font-medium">{title}</p>
                          <p className="text-micro text-foreground-muted font-mono">
                            {i === 0 ? '+28 -4' : i === 1 ? '+12 -3' : '+25 -1'}
                          </p>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              </TabsContent>

              <TabsContent value="activity" className="mt-0 min-h-0 flex-1">
                <ActivityContent stages={stages} />
              </TabsContent>
            </Tabs>
          </div>
        </div>

        <style>{`
          @keyframes pulse {
            0%, 100% { opacity: 1; }
            50% { opacity: 0.4; }
          }
        `}</style>
      </div>
    </TooltipProvider>
  );
}

/* ---------- Stories ---------- */

export const Overview: Story = {
  render: () => (
    <RunDetailPage repo="airlock-hq/airlock" branch="feat/auth-flow" status="completed" stages={completedStages} />
  ),
};

export const ActivityView: Story = {
  name: 'Activity',
  render: () => (
    <RunDetailPage
      repo="airlock-hq/airlock"
      branch="feat/auth-flow"
      status="running"
      stages={runningStages}
      defaultTab="activity"
    />
  ),
};

export const AwaitingApproval: Story = {
  render: () => (
    <RunDetailPage
      repo="airlock-hq/airlock"
      branch="fix/login-bug"
      status="awaiting_approval"
      stages={awaitingStages}
      defaultTab="activity"
    />
  ),
};

export const Failed: Story = {
  render: () => (
    <RunDetailPage
      repo="airlock-hq/airlock"
      branch="feat/k8s-deploy"
      status="failed"
      stages={failedStages}
      error='Pipeline stage "test" failed with exit code 1: assertion error in test_deploy_config'
      defaultTab="activity"
    />
  ),
};
