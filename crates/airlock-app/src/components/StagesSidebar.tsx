import { Button, StatusDot } from '@airlock-hq/design-system/react';
import type { StepResultInfo, JobResultInfo } from '@/hooks/use-daemon';
import { CheckCircle2, ChevronDown, ChevronRight, Loader2 } from 'lucide-react';
import { cn } from '@/lib/utils';
import { getStatusConfig } from '@/lib/status-utils';
import { useState } from 'react';
import { formatStageName, formatDuration } from '@/components/stage-log-viewer/utils';

/** Identifier for a selected step: job key + step name */
export interface StepSelection {
  jobKey: string;
  stepName: string;
}

interface StagesSidebarProps {
  jobs: JobResultInfo[];
  steps: StepResultInfo[];
  selectedStep: StepSelection | null;
  onSelectStep: (selection: StepSelection) => void;
  onApprove?: (jobKey: string, stepName: string) => Promise<void>;
  approvingStep?: StepSelection | null;
}

/**
 * StagesSidebar displays a vertical list of pipeline jobs and steps with status icons.
 * For single-job workflows, steps are shown flat without a job wrapper.
 * For multi-job workflows, jobs appear as collapsible sections with nested steps.
 */
export function StagesSidebar({
  jobs,
  steps,
  selectedStep,
  onSelectStep,
  onApprove,
  approvingStep,
}: StagesSidebarProps) {
  const isSingleJob = jobs.length <= 1;
  const [collapsedJobs, setCollapsedJobs] = useState<Set<string>>(new Set());

  const toggleJob = (jobKey: string) => {
    setCollapsedJobs((prev) => {
      const next = new Set(prev);
      if (next.has(jobKey)) {
        next.delete(jobKey);
      } else {
        next.add(jobKey);
      }
      return next;
    });
  };

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

  return (
    <div className="flex h-full flex-col">
      <div className="border-border-subtle border-b px-4 py-3">
        <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">
          {isSingleJob ? 'Steps' : 'Jobs'}
        </span>
      </div>
      <div className="flex-1 overflow-y-auto">
        {isSingleJob
          ? // Single job: flat list of steps (same as old behavior)
            steps.map((step) => (
              <StepItem
                key={`${step.job_key || 'default'}:${step.step}`}
                step={step}
                isSelected={
                  selectedStep?.jobKey === (step.job_key || 'default') && selectedStep?.stepName === step.step
                }
                onSelect={() => onSelectStep({ jobKey: step.job_key || 'default', stepName: step.step })}
                onApprove={onApprove}
                approvingStep={approvingStep}
              />
            ))
          : // Multi-job: collapsible job sections with nested steps
            sortedJobs.map((job) => {
              const jobSteps = stepsByJob.get(job.job_key) || [];
              const isCollapsed = collapsedJobs.has(job.job_key);

              return (
                <div key={job.job_key}>
                  {/* Job header */}
                  <div
                    className="border-border-subtle flex cursor-pointer items-center gap-2 border-b px-3 py-2"
                    onClick={() => toggleJob(job.job_key)}
                  >
                    {isCollapsed ? (
                      <ChevronRight className="text-foreground-muted h-3.5 w-3.5 shrink-0" />
                    ) : (
                      <ChevronDown className="text-foreground-muted h-3.5 w-3.5 shrink-0" />
                    )}
                    {getStatusDot(job.status)}
                    <span className="text-small min-w-0 flex-1 truncate font-medium">
                      {job.name || formatStageName(job.job_key)}
                    </span>
                  </div>

                  {/* Steps within this job */}
                  {!isCollapsed &&
                    jobSteps.map((step) => (
                      <StepItem
                        key={`${job.job_key}:${step.step}`}
                        step={step}
                        isSelected={selectedStep?.jobKey === job.job_key && selectedStep?.stepName === step.step}
                        onSelect={() => onSelectStep({ jobKey: job.job_key, stepName: step.step })}
                        onApprove={onApprove}
                        approvingStep={approvingStep}
                        indent={true}
                      />
                    ))}
                </div>
              );
            })}
      </div>
    </div>
  );
}

interface StepItemProps {
  step: StepResultInfo;
  isSelected: boolean;
  onSelect: () => void;
  onApprove?: (jobKey: string, stepName: string) => Promise<void>;
  approvingStep?: StepSelection | null;
  indent?: boolean;
}

function StepItem({ step, isSelected, onSelect, onApprove, approvingStep, indent }: StepItemProps) {
  const isAwaitingApproval = step.status === 'awaiting_approval';
  const isActionable = isAwaitingApproval && onApprove;
  const jobKey = step.job_key || 'default';
  const isApproving = approvingStep?.jobKey === jobKey && approvingStep?.stepName === step.step;

  return (
    <div
      className={cn(
        'cursor-pointer py-3 transition-colors',
        indent ? 'pr-4 pl-8' : 'px-4',
        isSelected ? 'border-l-signal bg-surface/30 border-l-2' : 'hover:bg-surface/20 border-l-2 border-l-transparent'
      )}
      onClick={onSelect}
    >
      <div className="flex items-center gap-2.5">
        {getStatusDot(step.status)}
        <div className="min-w-0 flex-1">
          <div className="flex items-center justify-between gap-2">
            <span className="text-small truncate">{formatStageName(step.step)}</span>
            {step.duration_ms != null && (
              <span className="text-micro text-foreground-muted shrink-0 font-mono">
                {formatDuration(step.duration_ms)}
              </span>
            )}
          </div>
          {step.error && <p className="text-micro text-danger mt-0.5 truncate">{step.error}</p>}
        </div>
      </div>

      {/* Approve button for awaiting_approval steps */}
      {isActionable && (
        <div className="mt-2">
          <Button
            size="sm"
            className="text-micro h-7 w-full"
            onClick={(e: { stopPropagation: () => void }) => {
              e.stopPropagation();
              onApprove(jobKey, step.step);
            }}
            disabled={isApproving}
          >
            {isApproving ? (
              <Loader2 className="mr-1 h-3 w-3 animate-spin" />
            ) : (
              <CheckCircle2 className="mr-1 h-3 w-3" />
            )}
            Approve
          </Button>
        </div>
      )}
    </div>
  );
}

function getStatusDot(status: string) {
  const config = getStatusConfig(status);
  return <StatusDot variant={config.variant} size="md" pulse={config.pulse} />;
}
