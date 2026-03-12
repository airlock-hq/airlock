import { cn } from '@/lib/utils';
import { getStatusConfig } from '@/lib/status-utils';
import { formatStageName } from '@/components/stage-log-viewer/utils';
import type { StepResultInfo } from '@/hooks/use-daemon';

interface StepProgressBarProps {
  steps: StepResultInfo[];
  onStepClick: (jobKey: string, stepName: string) => void;
}

function formatStatusLabel(status: string): string {
  switch (status) {
    case 'passed':
      return 'Passed';
    case 'failed':
      return 'Failed';
    case 'running':
      return 'Running';
    case 'awaiting_approval':
      return 'Awaiting Approval';
    case 'skipped':
      return 'Skipped';
    case 'pending':
      return 'Pending';
    default:
      return status.charAt(0).toUpperCase() + status.slice(1).replace('_', ' ');
  }
}

export function StepProgressBar({ steps, onStepClick }: StepProgressBarProps) {
  if (steps.length === 0) return null;

  return (
    <div className="pt-3 pb-2">
      <div className="flex gap-1">
        {steps.map((step, i) => {
          const config = getStatusConfig(step.status);
          const isAnimated = step.status === 'running' || step.status === 'awaiting_approval';
          return (
            <button
              key={`${step.job_key || 'default'}-${step.step}-${i}`}
              className={cn(
                'h-2 min-w-0 flex-1 cursor-pointer rounded-full',
                !isAnimated && 'hover:opacity-80',
                config.barColor
              )}
              onClick={() => onStepClick(step.job_key || 'default', step.step)}
              title={`${formatStageName(step.step)} — ${formatStatusLabel(step.status)}`}
            />
          );
        })}
      </div>
    </div>
  );
}
