import type { StepResultInfo, JobResultInfo } from '@/hooks/use-daemon';
import { PipelineChart } from './PipelineChart';

interface OverviewTabProps {
  jobs: JobResultInfo[];
  steps: StepResultInfo[];
  onStepClick: (jobKey: string, stepName: string) => void;
}

export function OverviewTab({ jobs, steps, onStepClick }: OverviewTabProps) {
  return (
    <div className="h-full overflow-y-auto p-8">
      <PipelineChart jobs={jobs} steps={steps} onStepClick={onStepClick} />
    </div>
  );
}
