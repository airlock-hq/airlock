import { StatusDot, Badge } from '@airlock-hq/design-system/react';
import { Loader2 } from 'lucide-react';
import { getStatusConfig } from '@/lib/status-utils';
import { formatStageName, formatDuration } from '@/components/stage-log-viewer/utils';
import type { FeedEvent } from './types';

type StepEvent = Extract<FeedEvent, { type: 'step-running' | 'step-completed' | 'step-awaiting' }>;

interface StepItemProps {
  event: StepEvent;
  onClick?: () => void;
}

function getStatusLabel(status: string): string {
  switch (status) {
    case 'passed':
      return 'PASSED';
    case 'failed':
      return 'FAILED';
    case 'skipped':
      return 'SKIPPED';
    default:
      return status.toUpperCase();
  }
}

function getStatusLabelColor(status: string): string {
  switch (status) {
    case 'passed':
      return 'text-success';
    case 'failed':
      return 'text-danger';
    case 'skipped':
      return 'text-foreground-muted';
    default:
      return 'text-foreground-muted';
  }
}

export function StepItem({ event, onClick }: StepItemProps) {
  const { step } = event;
  const config = getStatusConfig(step.status);

  if (event.type === 'step-running') {
    return (
      <div className="hover:bg-surface/40 flex cursor-pointer items-center gap-3 px-4 py-2.5" onClick={onClick}>
        <StatusDot variant={config.variant} pulse />
        <span className="min-w-0 flex-1 truncate font-medium">{formatStageName(step.step)}</span>
        <Loader2 className="text-warning h-4 w-4 animate-spin" />
      </div>
    );
  }

  if (event.type === 'step-awaiting') {
    return (
      <div className="hover:bg-surface/40 flex cursor-pointer items-center gap-3 px-4 py-2.5" onClick={onClick}>
        <StatusDot variant={config.variant} pulse />
        <span className="min-w-0 flex-1 truncate font-medium">{formatStageName(step.step)}</span>
        <Badge variant="signal">Awaiting Approval</Badge>
      </div>
    );
  }

  // step-completed
  return (
    <div className="hover:bg-surface/40 flex cursor-pointer items-center gap-3 px-4 py-2.5" onClick={onClick}>
      <StatusDot variant={config.variant} />
      <span className="min-w-0 flex-1 truncate font-medium">{formatStageName(step.step)}</span>
      {step.duration_ms != null && (
        <span className="text-small text-foreground-muted shrink-0 font-mono">{formatDuration(step.duration_ms)}</span>
      )}
      <span className={`text-small shrink-0 font-mono uppercase ${getStatusLabelColor(step.status)}`}>
        {getStatusLabel(step.status)}
      </span>
    </div>
  );
}
