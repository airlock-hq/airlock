import type { Meta, StoryObj } from '@storybook/react';
import { Badge } from '../react/badge';
import { Button } from '../react/button';
import { Separator } from '../react/separator';
import { StatusDot } from '../react/status-dot';

const meta: Meta = {
  title: 'Patterns',
};

export default meta;
type Story = StoryObj;

/* ---------- Surfaces ---------- */

export const Surfaces: Story = {
  render: () => (
    <div className="flex flex-col gap-4">
      <p className="text-small font-semibold">Surface Hierarchy</p>
      <div className="flex gap-4">
        <div className="border-border bg-background flex h-32 w-48 items-center justify-center rounded-lg border">
          <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">background</span>
        </div>
        <div className="border-border bg-surface flex h-32 w-48 items-center justify-center rounded-lg border">
          <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">surface</span>
        </div>
        <div className="border-border bg-surface-elevated flex h-32 w-48 items-center justify-center rounded-lg border">
          <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">elevated</span>
        </div>
      </div>
      <p className="text-small font-semibold">Terminal Surface</p>
      <div className="border-terminal-border bg-terminal flex h-32 w-full items-center justify-center rounded-lg border">
        <span className="text-micro text-terminal-foreground font-mono tracking-widest uppercase">terminal</span>
      </div>
    </div>
  ),
};

/* ---------- Telemetry Labels ---------- */

export const TelemetryLabels: Story = {
  render: () => (
    <div className="flex flex-col gap-6">
      <p className="text-small font-semibold">Telemetry Style</p>
      <div className="border-border bg-surface rounded-lg border p-6">
        <div className="flex flex-col gap-3">
          {[
            ['SYSTEM STATUS', 'VALIDATING'],
            ['MODE', 'LOCAL_EXECUTION'],
            ['SYNC', 'ACTIVE'],
            ['PIPELINE', 'STAGE 3/5'],
          ].map(([label, value]) => (
            <div key={label} className="flex items-center gap-3">
              <span className="text-micro text-foreground-muted w-40 font-mono tracking-widest uppercase">{label}</span>
              <span className="text-micro text-foreground-muted">&mdash;</span>
              <span className="text-micro text-foreground font-mono tracking-widest uppercase">{value}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  ),
};

/* ---------- Workflow Row States ---------- */

const stages = [
  { name: 'lint', status: 'success' as const, duration: '2.1s' },
  { name: 'test', status: 'success' as const, duration: '14.3s' },
  { name: 'describe', status: 'running' as const, duration: '—' },
  { name: 'create-pr', status: 'queued' as const, duration: '—' },
  { name: 'push', status: 'queued' as const, duration: '—' },
];

function WorkflowDot({ status }: { status: 'success' | 'running' | 'queued' | 'failed' }) {
  const props: Record<string, { variant: 'success' | 'signal' | 'muted' | 'danger'; pulse?: boolean }> = {
    success: { variant: 'success' },
    running: { variant: 'signal', pulse: true },
    queued: { variant: 'muted' },
    failed: { variant: 'danger' },
  };
  const { variant, pulse } = props[status] || { variant: 'muted' as const };
  return <StatusDot variant={variant} size="md" pulse={pulse} />;
}

function badgeVariant(status: string) {
  if (status === 'success') return 'success' as const;
  if (status === 'running') return 'signal' as const;
  if (status === 'failed') return 'danger' as const;
  return 'default' as const;
}

export const WorkflowRows: Story = {
  render: () => (
    <div className="w-[480px]">
      <p className="text-small mb-4 font-semibold">Workflow Row States</p>
      <div className="border-border rounded-lg border">
        {stages.map((stage, i) => (
          <div key={stage.name}>
            {i > 0 && <Separator />}
            <div
              className={`flex items-center gap-3 px-4 py-3 ${stage.status === 'running' ? 'border-l-signal border-l-2' : ''}`}
            >
              <WorkflowDot status={stage.status} />
              <span className="text-small flex-1">{stage.name}</span>
              <Badge variant={badgeVariant(stage.status)}>{stage.status}</Badge>
              <span className="text-micro text-foreground-muted w-12 text-right font-mono">{stage.duration}</span>
            </div>
          </div>
        ))}
      </div>
      <p className="text-small mt-8 mb-4 font-semibold">Failed State</p>
      <div className="border-border rounded-lg border">
        {[
          { name: 'lint', status: 'success' as const, duration: '2.1s' },
          { name: 'test', status: 'failed' as const, duration: '8.7s' },
          { name: 'describe', status: 'queued' as const, duration: '—' },
        ].map((stage, i) => (
          <div key={stage.name}>
            {i > 0 && <Separator />}
            <div className="flex items-center gap-3 px-4 py-3">
              <WorkflowDot status={stage.status} />
              <span className="text-small flex-1">{stage.name}</span>
              <Badge variant={badgeVariant(stage.status)}>{stage.status}</Badge>
              <span className="text-micro text-foreground-muted w-12 text-right font-mono">{stage.duration}</span>
            </div>
          </div>
        ))}
      </div>
    </div>
  ),
};

/* ---------- Empty & Error States ---------- */

export const EmptyAndErrorStates: Story = {
  render: () => (
    <div className="flex w-[480px] flex-col gap-8">
      <p className="text-small font-semibold">Empty State</p>
      <div className="border-border-subtle bg-surface flex flex-col items-center gap-4 rounded-lg border py-16">
        <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">NO RUNS YET</span>
        <p className="text-small text-foreground-muted">Push to an enrolled repository to start a pipeline run.</p>
        <Button variant="outline" size="sm">
          Enroll Repository
        </Button>
      </div>
      <p className="text-small font-semibold">Error State</p>
      <div className="border-danger/30 bg-danger/5 flex flex-col items-center gap-4 rounded-lg border py-16">
        <span className="text-micro text-danger font-mono tracking-widest uppercase">CONNECTION FAILED</span>
        <p className="text-small text-danger">Unable to connect to the Airlock daemon.</p>
        <Button variant="danger" size="sm">
          Retry
        </Button>
      </div>
    </div>
  ),
};
