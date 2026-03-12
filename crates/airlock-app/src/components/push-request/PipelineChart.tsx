import { StatusDot } from '@airlock-hq/design-system/react';
import type { StepResultInfo, JobResultInfo } from '@/hooks/use-daemon';
import { cn } from '@/lib/utils';
import { getStatusConfig } from '@/lib/status-utils';
import { formatStageName, formatDuration } from '@/components/stage-log-viewer/utils';

interface PipelineChartProps {
  jobs: JobResultInfo[];
  steps: StepResultInfo[];
  onStepClick: (jobKey: string, stepName: string) => void;
}

export function PipelineChart({ jobs, steps, onStepClick }: PipelineChartProps) {
  const isSingleJob = jobs.length <= 1;

  // Group steps by job key
  const stepsByJob = new Map<string, StepResultInfo[]>();
  for (const step of steps) {
    const key = step.job_key || 'default';
    const existing = stepsByJob.get(key) || [];
    existing.push(step);
    stepsByJob.set(key, existing);
  }

  // Sort jobs by job_order
  const sortedJobs = [...jobs].sort((a, b) => a.job_order - b.job_order);

  // Find max duration across all steps for proportional bar widths
  const maxDuration = Math.max(...steps.map((s) => s.duration_ms ?? 0), 1);
  const allPending = steps.every((s) => s.duration_ms == null);

  if (steps.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-12 text-center">
        <p className="text-foreground-muted">Pipeline has not started.</p>
      </div>
    );
  }

  const renderStep = (step: StepResultInfo) => {
    const jobKey = step.job_key || 'default';
    const barPercent = allPending ? 100 : Math.max((step.duration_ms ?? 0) / maxDuration, 0.08) * 100;

    return (
      <div
        key={`${jobKey}:${step.step}`}
        className="group hover:bg-surface/40 flex cursor-pointer items-center gap-3 rounded px-2 py-1.5 transition-colors"
        onClick={() => onStepClick(jobKey, step.step)}
      >
        <span className="text-small w-20 shrink-0 truncate text-right font-medium">{formatStageName(step.step)}</span>
        <div className="min-w-0 flex-1">
          <div
            className={cn('h-5 rounded-sm transition-all', getStatusConfig(step.status).barColor)}
            style={{ width: `${barPercent}%`, minWidth: '1.5rem' }}
          />
        </div>
        <span className="text-micro text-foreground-muted w-16 shrink-0 text-right font-mono">
          {step.duration_ms != null ? formatDuration(step.duration_ms) : '--'}
        </span>
      </div>
    );
  };

  return (
    <div className="space-y-1">
      <h3 className="text-micro text-foreground-muted mb-3 font-mono tracking-widest uppercase">Pipeline</h3>
      <div className="border-border-subtle border-t pt-3">
        {isSingleJob ? (
          <div className="space-y-0.5">{steps.map(renderStep)}</div>
        ) : (
          <div className="space-y-3">
            {sortedJobs.map((job) => {
              const jobSteps = stepsByJob.get(job.job_key) || [];
              return (
                <div key={job.job_key}>
                  <div className="mb-1 flex items-center gap-2 px-2">
                    <StatusDot
                      variant={getStatusConfig(job.status).variant}
                      size="md"
                      pulse={getStatusConfig(job.status).pulse}
                    />
                    <span className="text-small font-medium">{job.name || formatStageName(job.job_key)}</span>
                  </div>
                  <div className="space-y-0.5 pl-2">{jobSteps.map(renderStep)}</div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
