import { Button, StatusDot } from '@airlock-hq/design-system/react';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@airlock-hq/design-system/react';
import { ActivityTab, OverviewTab, ChangesTab, PatchesTab, ContentTab } from '@/components/push-request';
import type { StepSelection } from '@/components/StagesSidebar';
import { useRunDetail, useRepos, getRepoNameFromUrl, reprocessRun, approveStep } from '@/hooks/use-daemon';
import {
  Loader2,
  RefreshCw,
  GitCommit,
  FileText,
  FileDiff,
  Layers,
  Activity,
  CheckCircle2,
  BookOpen,
} from 'lucide-react';
import { useParams, useSearchParams } from 'react-router-dom';
import { useState, useCallback, useMemo } from 'react';

function getRunStatusDotProps(status: string): {
  variant: 'success' | 'danger' | 'warning' | 'signal' | 'muted';
  pulse?: boolean;
} {
  switch (status) {
    case 'running':
      return { variant: 'warning' };
    case 'pending_review':
    case 'awaiting_approval':
      return { variant: 'signal', pulse: true };
    case 'completed':
    case 'forwarded':
      return { variant: 'success' };
    case 'failed':
      return { variant: 'danger' };
    case 'superseded':
      return { variant: 'muted' };
    default:
      return { variant: 'muted' };
  }
}

function getStatusTextColor(status: string): string {
  switch (status) {
    case 'completed':
    case 'forwarded':
      return 'text-success';
    case 'failed':
      return 'text-danger';
    case 'running':
      return 'text-warning';
    case 'pending_review':
    case 'awaiting_approval':
      return 'text-signal';
    case 'superseded':
    default:
      return 'text-foreground-muted';
  }
}

export function RunDetail() {
  const { repoId, runId } = useParams<{ repoId: string; runId: string }>();
  const { detail, loading, error, refresh } = useRunDetail(runId ?? null);
  const { repos } = useRepos();
  const repoName = useMemo(() => {
    const repo = repos.find((r) => r.id === repoId);
    return repo ? getRepoNameFromUrl(repo.upstream_url) : undefined;
  }, [repos, repoId]);
  const [reprocessing, setReprocessing] = useState(false);
  const [approvingStep, setApprovingStep] = useState<StepSelection | null>(null);

  const handleReprocess = async () => {
    if (!runId) return;
    try {
      setReprocessing(true);
      await reprocessRun(runId);
      await refresh();
    } catch (e) {
      console.error('Reprocess failed:', e);
    } finally {
      setReprocessing(false);
    }
  };

  const handleApproveStep = useCallback(
    async (jobKey: string, stepName: string) => {
      if (!runId) return;
      try {
        setApprovingStep({ jobKey, stepName });
        await approveStep(runId, jobKey, stepName);
        await refresh();
      } catch (e) {
        console.error('Approve step failed:', e);
      } finally {
        setApprovingStep(null);
      }
    },
    [runId, refresh]
  );

  const formatStatusLabel = (status: string): string => {
    switch (status) {
      case 'pending_review':
      case 'awaiting_approval':
        return 'Awaiting Approval';
      case 'running':
        return 'Running';
      case 'completed':
        return 'Completed';
      case 'forwarded':
        return 'Forwarded';
      case 'failed':
        return 'Failed';
      case 'superseded':
        return 'Superseded';
      default:
        return status.charAt(0).toUpperCase() + status.slice(1).replace('_', ' ');
    }
  };

  // Count pending patches for tab badge
  const pendingPatchCount = useMemo(() => {
    if (!detail?.artifacts) return 0;
    const patchArtifacts = detail.artifacts.filter((a) => a.path.includes('/patches/') && a.path.endsWith('.json'));

    const freezeStep = detail.step_results.find((s) => s.step === 'freeze' && s.status === 'passed');
    const freezeCompletedAt = freezeStep?.completed_at;

    return patchArtifacts.filter((a) => freezeCompletedAt == null || a.created_at > freezeCompletedAt).length;
  }, [detail?.artifacts, detail?.step_results]);

  // Store active tab in URL search params so it survives refreshes and is shareable
  const [searchParams, setSearchParams] = useSearchParams();
  const tabParam = searchParams.get('tab');

  // Compute the active tab: use URL param if valid, otherwise default to overview
  const validTabs = ['overview', 'content', 'activity', 'changes', 'patches'];
  const activeTab = tabParam && validTabs.includes(tabParam) ? tabParam : 'overview';

  const setActiveTab = useCallback(
    (tab: string) => {
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          next.set('tab', tab);
          return next;
        },
        { replace: true }
      );
    },
    [setSearchParams]
  );

  return (
    <div className="flex h-full flex-col gap-4">
      {/* Header */}
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-h2 text-foreground font-bold tracking-tight">
            {detail?.run.branch || `Run #${runId?.slice(-8)}`}
          </h1>

          {/* Metadata */}
          {detail?.run && (
            <div className="text-micro text-foreground-muted mt-2 flex items-center gap-3 font-mono">
              <span className={`inline-flex items-center gap-1.5 ${getStatusTextColor(detail.run.status)}`}>
                <StatusDot {...getRunStatusDotProps(detail.run.status)} />
                <span className="uppercase">{formatStatusLabel(detail.run.status)}</span>
              </span>
              <span>&middot;</span>
              <div className="flex items-center gap-1.5">
                <GitCommit className="h-3 w-3" />
                <span>{repoName || detail.run.branch || repoId}</span>
              </div>
              <span>&middot;</span>
              <span>{formatTime(detail.run.created_at)}</span>
            </div>
          )}
        </div>

        <div className="flex items-center gap-2">
          {detail?.run.status === 'awaiting_approval' &&
            (() => {
              const awaitingStep = detail.step_results.find((s) => s.status === 'awaiting_approval');
              if (!awaitingStep) return null;
              const jobKey = awaitingStep.job_key || 'default';
              const isApproving = approvingStep?.jobKey === jobKey && approvingStep?.stepName === awaitingStep.step;
              return (
                <Button
                  variant="signal"
                  size="sm"
                  onClick={() => handleApproveStep(jobKey, awaitingStep.step)}
                  disabled={isApproving}
                >
                  {isApproving ? (
                    <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <CheckCircle2 className="mr-1.5 h-3.5 w-3.5" />
                  )}
                  Approve
                </Button>
              );
            })()}
          {detail?.run.status !== 'running' && (
            <Button
              variant="ghost"
              size="sm"
              className="border-border-subtle"
              onClick={handleReprocess}
              disabled={reprocessing}
            >
              {reprocessing ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <RefreshCw className="mr-1.5 h-3.5 w-3.5" />
              )}
              Rerun
            </Button>
          )}
        </div>
      </div>

      {/* Error display */}
      {error && (
        <div className="border-danger/20 bg-danger/5 rounded-md border px-4 py-2.5">
          <p className="text-small text-danger">{error}</p>
        </div>
      )}

      {/* Run error display */}
      {detail?.run.error && (
        <div className="border-danger/20 bg-danger/5 rounded-md border px-4 py-2.5">
          <p className="text-small text-danger">{detail.run.error}</p>
        </div>
      )}

      {/* Main content area with tabs */}
      <div className="border-border-subtle bg-background/60 flex min-h-0 flex-1 flex-col rounded-lg border">
        {loading && !detail ? (
          <div className="flex flex-1 items-center justify-center">
            <Loader2 className="text-foreground-muted h-8 w-8 animate-spin" />
          </div>
        ) : detail ? (
          <Tabs value={activeTab} onValueChange={setActiveTab} className="flex min-h-0 flex-1 flex-col">
            <TabsList variant="line" className="w-full justify-start px-3 pt-2">
              <TabsTrigger variant="line" value="overview">
                <FileText className="mr-2 h-4 w-4" />
                Overview
              </TabsTrigger>
              <TabsTrigger variant="line" value="content">
                <BookOpen className="mr-2 h-4 w-4" />
                Content
              </TabsTrigger>
              <TabsTrigger variant="line" value="activity">
                <Activity className="mr-2 h-4 w-4" />
                Activity
              </TabsTrigger>
              <TabsTrigger variant="line" value="changes">
                <FileDiff className="mr-2 h-4 w-4" />
                Changes
              </TabsTrigger>
              <TabsTrigger variant="line" value="patches">
                <Layers className="mr-2 h-4 w-4" />
                Patches
                {pendingPatchCount > 0 && (
                  <span className="bg-signal/20 text-micro ml-2 rounded-full px-2 py-0.5">{pendingPatchCount}</span>
                )}
              </TabsTrigger>
            </TabsList>

            <TabsContent value="overview" className="mt-0 min-h-0 flex-1">
              <OverviewTab
                jobs={detail.jobs}
                steps={detail.step_results}
                onStepClick={(jobKey, stepName) => {
                  setSearchParams(
                    (prev) => {
                      const next = new URLSearchParams(prev);
                      next.set('tab', 'activity');
                      next.set('job', jobKey);
                      next.set('step', stepName);
                      return next;
                    },
                    { replace: true }
                  );
                }}
              />
            </TabsContent>

            <TabsContent value="activity" className="mt-0 min-h-0 flex-1">
              <ActivityTab
                repoId={repoId!}
                runId={runId!}
                jobs={detail.jobs}
                steps={detail.step_results}
                artifacts={detail.artifacts}
                onApproveStep={handleApproveStep}
                approvingStep={approvingStep}
              />
            </TabsContent>

            <TabsContent value="content" className="mt-0 min-h-0 flex-1">
              <ContentTab artifacts={detail.artifacts} />
            </TabsContent>

            <TabsContent value="changes" className="mt-0 min-h-0 flex-1">
              <ChangesTab runId={runId!} artifacts={detail.artifacts} />
            </TabsContent>

            <TabsContent value="patches" className="mt-0 min-h-0 flex-1">
              <PatchesTab
                artifacts={detail.artifacts}
                stepResults={detail.step_results}
                runId={runId!}
                onPatchesApplied={refresh}
              />
            </TabsContent>
          </Tabs>
        ) : (
          <div className="flex flex-1 items-center justify-center">
            <p className="text-foreground-muted">Run not found</p>
          </div>
        )}
      </div>
    </div>
  );
}

function formatTime(timestamp: number): string {
  const now = Date.now() / 1000;
  const diff = now - timestamp;

  if (diff < 60) return 'just now';
  if (diff < 3600) {
    const mins = Math.floor(diff / 60);
    return `${mins}m ago`;
  }
  if (diff < 86400) {
    const hours = Math.floor(diff / 3600);
    return `${hours}h ago`;
  }
  if (diff < 604800) {
    const days = Math.floor(diff / 86400);
    if (days === 1) return 'yesterday';
    return `${days} days ago`;
  }
  const weeks = Math.floor(diff / 604800);
  return `${weeks} week${weeks !== 1 ? 's' : ''} ago`;
}
