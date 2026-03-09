import { Button, StatusDot } from '@airlock-hq/design-system/react';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@airlock-hq/design-system/react';
import { ActivityTab, OverviewTab, ChangesTab, PatchesTab, ContentTab } from '@/components/push-request';
import { StepProgressBar } from '@/components/push-request/activity-feed/StepProgressBar';
import type { StepSelection } from '@/components/StagesSidebar';
import {
  useRunDetail,
  useRepos,
  getRepoNameFromUrl,
  reprocessRun,
  cancelRun,
  approveStep,
  readArtifact,
  applyPatches,
} from '@/hooks/use-daemon';
import type { PatchArtifact } from '@/components/push-request/PatchesTab';
import { getCommentKey, getPatchId, type CodeComment } from '@/lib/artifact-keys';
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
  Copy,
  Check,
  Square,
  ExternalLink,
} from 'lucide-react';
import { useParams, useSearchParams } from 'react-router-dom';
import { useState, useCallback, useMemo, useEffect } from 'react';

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
  const [cancelling, setCancelling] = useState(false);
  const [approvingStep, setApprovingStep] = useState<StepSelection | null>(null);
  const [allComments, setAllComments] = useState<CodeComment[]>([]);
  const [selectedComments, setSelectedComments] = useState<Set<string>>(new Set());
  const [allPatches, setAllPatches] = useState<PatchArtifact[]>([]);
  const [selectedPatches, setSelectedPatches] = useState<Set<string>>(new Set());
  const [prUrl, setPrUrl] = useState<string | null>(null);
  const [artifactDataLoading, setArtifactDataLoading] = useState(false);
  const [applyingAndApproving, setApplyingAndApproving] = useState(false);
  const [commentsCopied, setCommentsCopied] = useState(false);

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

  const handleCancel = async () => {
    if (!runId) return;
    try {
      setCancelling(true);
      await cancelRun(runId);
      await refresh();
    } catch (e) {
      console.error('Cancel failed:', e);
    } finally {
      setCancelling(false);
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

  // Count content files for tab badge
  const contentCount = useMemo(() => {
    if (!detail?.artifacts) return 0;
    return detail.artifacts.filter(
      (a) => a.artifact_type === 'file' && a.path.includes('/content/') && a.path.endsWith('.md')
    ).length;
  }, [detail?.artifacts]);

  // Artifact file lists for loading
  const commentFiles = useMemo(
    () =>
      (detail?.artifacts ?? []).filter(
        (a) => a.artifact_type === 'file' && a.path.includes('/comments/') && a.path.endsWith('.json')
      ),
    [detail?.artifacts]
  );
  const patchFiles = useMemo(
    () =>
      (detail?.artifacts ?? []).filter(
        (a) => a.artifact_type === 'file' && a.path.includes('/patches/') && a.path.endsWith('.json')
      ),
    [detail?.artifacts]
  );

  // Load PR URL from pr_result.json artifact
  const prArtifact = useMemo(
    () => (detail?.artifacts ?? []).find((a) => a.artifact_type === 'pr'),
    [detail?.artifacts]
  );

  useEffect(() => {
    if (!prArtifact) {
      setPrUrl(null);
      return;
    }
    let cancelled = false;
    readArtifact(prArtifact.path)
      .then((result) => {
        if (cancelled) return;
        try {
          const parsed = JSON.parse(result.content);
          if (parsed.success && parsed.pr_url) {
            setPrUrl(parsed.pr_url);
          } else {
            setPrUrl(null);
          }
        } catch {
          setPrUrl(null);
        }
      })
      .catch(() => {
        if (!cancelled) setPrUrl(null);
      });
    return () => {
      cancelled = true;
    };
  }, [prArtifact]);

  // Load comments and patches from artifact files
  useEffect(() => {
    let cancelled = false;

    if (commentFiles.length === 0 && patchFiles.length === 0) {
      setAllComments([]);
      setSelectedComments(new Set());
      setAllPatches([]);
      setSelectedPatches(new Set());
      setArtifactDataLoading(false);
      return;
    }

    setArtifactDataLoading(true);

    async function loadArtifactData() {
      const loadedComments: CodeComment[] = [];
      for (const file of commentFiles) {
        try {
          const result = await readArtifact(file.path);
          if (!result.is_binary) {
            const parsed = JSON.parse(result.content);
            if (Array.isArray(parsed.comments)) {
              loadedComments.push(...parsed.comments);
            }
          }
        } catch {
          // skip unreadable files
        }
      }

      const loadedPatches: PatchArtifact[] = [];
      for (const file of patchFiles) {
        try {
          const result = await readArtifact(file.path);
          if (!result.is_binary) {
            const parsed = JSON.parse(result.content);
            const id = getPatchId(file.path);
            const applied = file.path.includes('/patches/applied/');
            loadedPatches.push({
              id,
              title: parsed.title || 'Untitled Patch',
              explanation: parsed.explanation || '',
              diff: parsed.diff || '',
              applied,
              artifactPath: file.path,
            });
          }
        } catch {
          // skip unreadable files
        }
      }

      if (!cancelled) {
        setAllComments(loadedComments);
        setSelectedComments((prev) => {
          const prevKeys = new Set(prev);
          const allKeys = new Set(loadedComments.map(getCommentKey));
          // Keep existing selections that are still valid, add defaults for new items only
          const next = new Set<string>();
          for (const key of prevKeys) {
            if (allKeys.has(key)) next.add(key);
          }
          for (const c of loadedComments) {
            const key = getCommentKey(c);
            if (!prevKeys.has(key) && c.severity === 'error') next.add(key);
          }
          return next;
        });
        setAllPatches(loadedPatches);
        setSelectedPatches((prev) => {
          const prevIds = new Set(prev);
          const allIds = new Set(loadedPatches.map((p) => p.id));
          // Keep existing selections that are still valid, add defaults for new items only
          const next = new Set<string>();
          for (const id of prevIds) {
            if (allIds.has(id)) next.add(id);
          }
          for (const p of loadedPatches) {
            if (!prevIds.has(p.id) && !p.applied) next.add(p.id);
          }
          return next;
        });
        setArtifactDataLoading(false);
      }
    }

    loadArtifactData();
    return () => {
      cancelled = true;
    };
  }, [commentFiles, patchFiles]);

  // Derived counts
  const commentCount = allComments.length;
  const pendingPatchCount = useMemo(() => allPatches.filter((p) => !p.applied).length, [allPatches]);
  const selectedPendingPatchCount = useMemo(
    () => allPatches.filter((p) => !p.applied && selectedPatches.has(p.id)).length,
    [allPatches, selectedPatches]
  );

  // Comment toggle handler
  const handleToggleComment = useCallback((key: string) => {
    setSelectedComments((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  // Patch selection handlers
  const handleTogglePatch = useCallback((id: string) => {
    setSelectedPatches((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const handleSelectAllPatches = useCallback(() => {
    setSelectedPatches(new Set(allPatches.filter((p) => !p.applied).map((p) => p.id)));
  }, [allPatches]);

  const handleSelectNonePatches = useCallback(() => {
    setSelectedPatches(new Set());
  }, []);

  // Copy selected comments to clipboard as markdown
  const handleCopyComments = useCallback(async () => {
    const selected = allComments.filter((c) => selectedComments.has(getCommentKey(c)));
    const markdown = selected.map((c) => `- **[${c.severity}]** \`${c.file}:${c.line}\` — ${c.message}`).join('\n');
    await navigator.clipboard.writeText(markdown);
    setCommentsCopied(true);
    setTimeout(() => setCommentsCopied(false), 1500);
  }, [allComments, selectedComments]);

  // Apply selected patches then approve
  const handleApplyAndApprove = useCallback(async () => {
    const awaitingStep = detail?.step_results.find((s) => s.status === 'awaiting_approval');
    if (!awaitingStep || !runId) return;
    try {
      setApplyingAndApproving(true);
      const paths = allPatches.filter((p) => !p.applied && selectedPatches.has(p.id)).map((p) => p.artifactPath);
      if (paths.length > 0) {
        await applyPatches(runId, paths);
      }
      const jobKey = awaitingStep.job_key || 'default';
      await approveStep(runId, jobKey, awaitingStep.step);
      await refresh();
    } catch (e) {
      console.error('Apply & approve failed:', e);
    } finally {
      setApplyingAndApproving(false);
    }
  }, [detail?.step_results, runId, allPatches, selectedPatches, refresh]);

  // Store active tab in URL search params so it survives refreshes and is shareable
  const [searchParams, setSearchParams] = useSearchParams();
  const tabParam = searchParams.get('tab');

  // Compute the active tab: use URL param if valid, otherwise default to overview
  const validTabs = ['overview', 'changes', 'content', 'patches', 'activity'];
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
          {prUrl && (
            <Button variant="ghost" size="sm" className="border-border-subtle" asChild>
              <a href={prUrl} target="_blank" rel="noopener noreferrer">
                <ExternalLink className="mr-1.5 h-3.5 w-3.5" />
                View Pull Request
              </a>
            </Button>
          )}
          {detail?.run.status === 'running' ? (
            <Button
              variant="ghost"
              size="sm"
              className="border-border-subtle"
              onClick={handleCancel}
              disabled={cancelling}
            >
              {cancelling ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Square className="mr-1.5 h-3.5 w-3.5" />
              )}
              Stop
            </Button>
          ) : (
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

      {/* Step progress bar */}
      {detail && (
        <StepProgressBar
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
              <TabsTrigger variant="line" value="changes">
                <FileDiff className="mr-2 h-4 w-4" />
                Critique
                {commentCount > 0 && (
                  <span className="bg-signal/20 text-micro ml-2 rounded-full px-2 py-0.5">{commentCount}</span>
                )}
              </TabsTrigger>
              <TabsTrigger variant="line" value="content">
                <BookOpen className="mr-2 h-4 w-4" />
                Content
                {contentCount > 0 && (
                  <span className="bg-signal/20 text-micro ml-2 rounded-full px-2 py-0.5">{contentCount}</span>
                )}
              </TabsTrigger>
              <TabsTrigger variant="line" value="patches">
                <Layers className="mr-2 h-4 w-4" />
                Patches
                {pendingPatchCount > 0 && (
                  <span className="bg-signal/20 text-micro ml-2 rounded-full px-2 py-0.5">{pendingPatchCount}</span>
                )}
              </TabsTrigger>
              <TabsTrigger variant="line" value="activity">
                <Activity className="mr-2 h-4 w-4" />
                Activity
              </TabsTrigger>
            </TabsList>

            <TabsContent value="overview" className="mt-0 min-h-0 flex-1">
              <OverviewTab
                artifacts={detail.artifacts}
                selectedComments={selectedComments}
                onToggleComment={handleToggleComment}
                selectedPatches={selectedPatches}
                onTogglePatch={handleTogglePatch}
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
              <ChangesTab
                runId={runId!}
                artifacts={detail.artifacts}
                selectedComments={selectedComments}
                onToggleComment={handleToggleComment}
              />
            </TabsContent>

            <TabsContent value="patches" className="mt-0 min-h-0 flex-1">
              <PatchesTab
                patches={allPatches}
                patchesLoading={artifactDataLoading}
                selectedPatches={selectedPatches}
                onTogglePatch={handleTogglePatch}
                onSelectAllPatches={handleSelectAllPatches}
                onSelectNonePatches={handleSelectNonePatches}
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

      {/* Bottom action bar */}
      {detail?.run.status === 'awaiting_approval' && (
        <div className="border-border-subtle bg-background flex items-center justify-between rounded-lg border px-4 py-3">
          <button
            className="text-small text-foreground-muted hover:text-foreground cursor-pointer"
            onClick={() => {
              if (selectedComments.size === allComments.length) {
                setSelectedComments(new Set());
              } else {
                setSelectedComments(new Set(allComments.map(getCommentKey)));
              }
            }}
          >
            {selectedComments.size}/{allComments.length} {allComments.length === 1 ? 'comment' : 'comments'},{' '}
            {selectedPendingPatchCount}/{pendingPatchCount} {pendingPatchCount === 1 ? 'patch' : 'patches'} selected
          </button>

          <div className="flex items-center gap-2">
            <Button
              variant="signal-outline"
              size="sm"
              disabled={selectedComments.size === 0}
              onClick={handleCopyComments}
            >
              {commentsCopied ? (
                <>
                  <Check className="text-success mr-1.5 h-3.5 w-3.5" />
                  Copied!
                </>
              ) : (
                <>
                  <Copy className="mr-1.5 h-3.5 w-3.5" />
                  Copy {selectedComments.size} {selectedComments.size === 1 ? 'comment' : 'comments'}
                </>
              )}
            </Button>

            {selectedPendingPatchCount > 0 && (
              <Button
                variant="signal-outline"
                size="sm"
                disabled={applyingAndApproving}
                onClick={handleApplyAndApprove}
              >
                {applyingAndApproving ? (
                  <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Layers className="mr-1.5 h-3.5 w-3.5" />
                )}
                Apply {selectedPendingPatchCount} {selectedPendingPatchCount === 1 ? 'patch' : 'patches'} & approve
              </Button>
            )}

            <Button
              variant="signal"
              size="sm"
              disabled={approvingStep !== null || applyingAndApproving}
              onClick={() => {
                const awaitingStep = detail?.step_results.find((s) => s.status === 'awaiting_approval');
                if (awaitingStep) {
                  const jobKey = awaitingStep.job_key || 'default';
                  handleApproveStep(jobKey, awaitingStep.step);
                }
              }}
            >
              {approvingStep !== null ? (
                <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
              ) : (
                <CheckCircle2 className="mr-1.5 h-3.5 w-3.5" />
              )}
              Approve
            </Button>
          </div>
        </div>
      )}
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
