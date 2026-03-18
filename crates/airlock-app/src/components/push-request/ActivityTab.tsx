import { StagesSidebar } from '@/components/StagesSidebar';
import type { StepSelection } from '@/components/StagesSidebar';
import { StageLogViewer } from '@/components/stage-log-viewer';
import type { StepResultInfo, JobResultInfo, ArtifactInfo } from '@/hooks/use-daemon';
import { ResizablePanelGroup, ResizablePanel, ResizableHandle } from '@airlock-hq/design-system/react';
import { useCallback, useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';

interface ActivityTabProps {
  repoId: string;
  runId: string;
  jobs: JobResultInfo[];
  steps: StepResultInfo[];
  artifacts: ArtifactInfo[];
  onApproveStep?: (jobKey: string, stepName: string) => Promise<void>;
  approvingStep?: StepSelection | null;
  onRetryJob?: (jobKey: string) => Promise<void>;
  retryingJob?: string | null;
}

/**
 * ActivityTab displays the pipeline jobs/steps sidebar and log viewer.
 * URL params: ?job=<key>&step=<name>
 */
export function ActivityTab({
  repoId,
  runId,
  jobs,
  steps,
  artifacts,
  onApproveStep,
  approvingStep,
  onRetryJob,
  retryingJob,
}: ActivityTabProps) {
  // Store selected step in URL search params
  const [searchParams, setSearchParams] = useSearchParams();
  const jobParam = searchParams.get('job');
  const stepParam = searchParams.get('step');

  // Compute the effective selected step: use URL params if valid, otherwise first step
  const selectedStep: StepSelection | null = useMemo(() => {
    if (jobParam && stepParam && steps.find((s) => (s.job_key || 'default') === jobParam && s.step === stepParam)) {
      return { jobKey: jobParam, stepName: stepParam };
    }
    // Default to the first step
    if (steps.length > 0) {
      return { jobKey: steps[0].job_key || 'default', stepName: steps[0].step };
    }
    return null;
  }, [jobParam, stepParam, steps]);

  const handleSelectStep = useCallback(
    (selection: StepSelection) => {
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          next.set('job', selection.jobKey);
          next.set('step', selection.stepName);
          return next;
        },
        { replace: true }
      );
    },
    [setSearchParams]
  );

  // Find the selected step info
  const selectedStepInfo = useMemo(() => {
    if (!selectedStep) return null;
    return (
      steps.find((s) => (s.job_key || 'default') === selectedStep.jobKey && s.step === selectedStep.stepName) ?? null
    );
  }, [selectedStep, steps]);

  if (steps.length === 0) {
    return (
      <div className="flex h-full items-center justify-center">
        <p className="text-foreground-muted">No steps in this pipeline</p>
      </div>
    );
  }

  return (
    <ResizablePanelGroup direction="horizontal" autoSaveId="activity-panels">
      <ResizablePanel defaultSize={20} minSize={12} maxSize={35}>
        <StagesSidebar
          jobs={jobs}
          steps={steps}
          selectedStep={selectedStep}
          onSelectStep={handleSelectStep}
          onApprove={onApproveStep}
          approvingStep={approvingStep}
          onRetryJob={onRetryJob}
          retryingJob={retryingJob}
        />
      </ResizablePanel>

      <ResizableHandle />

      <ResizablePanel defaultSize={80} minSize={50}>
        <div className="h-full overflow-y-auto">
          {selectedStepInfo && selectedStep ? (
            <StageLogViewer
              step={selectedStepInfo}
              jobKey={selectedStep.jobKey}
              repoId={repoId}
              runId={runId}
              artifacts={artifacts}
            />
          ) : (
            <div className="flex h-full items-center justify-center">
              <p className="text-foreground-muted">Select a step to view logs</p>
            </div>
          )}
        </div>
      </ResizablePanel>
    </ResizablePanelGroup>
  );
}
