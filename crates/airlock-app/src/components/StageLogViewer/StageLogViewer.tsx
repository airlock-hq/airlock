import { useState, useEffect } from 'react';
import { Button } from '@airlock-hq/design-system/react';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@airlock-hq/design-system/react';
import type { StepResultInfo, ArtifactInfo } from '@/hooks/use-daemon';
import { useStageLog } from '@/hooks/use-stage-log';
import { Loader2, FileText, AlertCircle, Terminal, Search, X, FileJson, AlertTriangle, File } from 'lucide-react';
import { Input } from '@airlock-hq/design-system/react';
import { LogContent } from './LogContent';
import { ArtifactContent } from './ArtifactContent';
import { MarkdownViewer } from './MarkdownViewer';
import { formatStageName, formatDuration, buildTabItems, getFileExtension, type TabItem } from './utils';

interface StageLogViewerProps {
  step: StepResultInfo;
  jobKey: string;
  repoId: string;
  runId: string;
  artifacts?: ArtifactInfo[];
}

/**
 * Get icon for a tab item based on its type and file extension.
 */
function TabIcon({ item }: { item: TabItem }) {
  if (item.type === 'log') {
    if (item.logType === 'stderr') {
      return <AlertTriangle className="h-4 w-4" />;
    }
    return <Terminal className="h-4 w-4" />;
  }

  // Artifact tabs
  const ext = item.artifact ? getFileExtension(item.artifact.name) : '';
  if (ext === 'md') {
    return <FileText className="h-4 w-4" />;
  }
  if (ext === 'json') {
    return <FileJson className="h-4 w-4" />;
  }
  return <File className="h-4 w-4" />;
}

/**
 * StageLogViewer displays logs (stdout/stderr) for a pipeline step.
 * Uses a tabbed interface to switch between different outputs.
 * Supports real-time updates for running steps.
 */
export function StageLogViewer({ step, jobKey, repoId, runId, artifacts = [] }: StageLogViewerProps) {
  const [selectedTab, setSelectedTab] = useState<string | null>(null);
  const [searchVisible, setSearchVisible] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');

  const { stdout, stderr, loading, error, isPolling } = useStageLog({
    repoId,
    runId,
    jobKey,
    stepName: step.step,
    isRunning: step.status === 'running',
  });

  // Filter artifacts for this step
  const stepArtifacts = artifacts.filter((a) => {
    // New path format: /{job_key}/{step_name}/ or legacy /{step_name}/
    const pathMatch = a.path.includes(`/${jobKey}/${step.step}/`) || a.path.includes(`/${step.step}/`);
    const typeMatch =
      a.artifact_type === step.step ||
      (step.step === 'describe' && (a.artifact_type === 'description' || a.artifact_type === 'analysis')) ||
      (step.step === 'test' && (a.artifact_type === 'test_results' || a.artifact_type === 'coverage')) ||
      (step.step === 'create-pr' && a.artifact_type === 'pr');
    return pathMatch || typeMatch;
  });

  // Build tab items sorted by priority
  const tabItems = buildTabItems(stepArtifacts, stdout.length > 0, stderr.length > 0);

  // Compute effective active tab: use selected if valid, otherwise default to first tab
  const activeTab = selectedTab && tabItems.some((t) => t.key === selectedTab) ? selectedTab : (tabItems[0]?.key ?? '');

  const hasLogs = stdout.length > 0 || stderr.length > 0;
  const isPending = step.status === 'pending';

  // Keyboard shortcut for search
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'f') {
        e.preventDefault();
        setSearchVisible(true);
      }
      if (e.key === 'Escape') {
        setSearchVisible(false);
        setSearchQuery('');
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  if (isPending) {
    return (
      <div className="text-foreground-muted flex h-full items-center justify-center">
        <div className="text-center">
          <FileText className="mx-auto h-12 w-12 opacity-20" />
          <p className="text-small mt-2">Step pending - no logs yet</p>
        </div>
      </div>
    );
  }

  if (loading && !hasLogs) {
    return (
      <div className="text-foreground-muted flex h-full items-center justify-center">
        <Loader2 className="h-6 w-6 animate-spin" />
        <span className="ml-2">Loading logs...</span>
      </div>
    );
  }

  if (error && !hasLogs) {
    return (
      <div className="text-foreground-muted flex h-full items-center justify-center">
        <div className="text-center">
          <AlertCircle className="text-danger/50 mx-auto h-12 w-12" />
          <p className="text-small text-danger mt-2">{error}</p>
        </div>
      </div>
    );
  }

  if (!hasLogs && stepArtifacts.length === 0) {
    return (
      <div className="text-foreground-muted flex h-full items-center justify-center">
        <div className="text-center">
          <Terminal className="mx-auto h-12 w-12 opacity-20" />
          <p className="text-small mt-2">No logs available for this step</p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <div className="border-border-subtle flex items-center justify-between border-b px-3 py-1">
        <div className="flex items-center gap-2">
          <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">
            {formatStageName(step.step)}
          </span>
          {step.duration_ms != null && (
            <span className="text-micro text-foreground-muted">{formatDuration(step.duration_ms)}</span>
          )}
          {isPolling && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
        </div>
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="icon" onClick={() => setSearchVisible(!searchVisible)}>
            <Search className="h-4 w-4" />
          </Button>
        </div>
      </div>

      {/* Search bar */}
      {searchVisible && (
        <div className="bg-surface/30 flex items-center gap-2 border-b px-4 py-2">
          <Search className="text-foreground-muted h-4 w-4" />
          <Input
            autoFocus
            placeholder="Search in logs..."
            value={searchQuery}
            onChange={(e: { target: { value: string } }) => setSearchQuery(e.target.value)}
            className="h-7 flex-1"
          />
          <Button
            variant="ghost"
            size="sm"
            className="h-7 w-7 p-0"
            onClick={() => {
              setSearchVisible(false);
              setSearchQuery('');
            }}
          >
            <X className="h-4 w-4" />
          </Button>
        </div>
      )}

      {/* Tabbed content */}
      <Tabs value={activeTab} onValueChange={setSelectedTab} className="flex flex-1 flex-col overflow-hidden">
        <div className="px-2 pt-2">
          <TabsList variant="line">
            {tabItems.map((item) => (
              <TabsTrigger key={item.key} value={item.key} variant="line" className="text-micro gap-1">
                <TabIcon item={item} />
                <span>{item.label}</span>
              </TabsTrigger>
            ))}
          </TabsList>
        </div>

        <div className="flex-1 overflow-hidden">
          {tabItems.map((item) => (
            <TabsContent key={item.key} value={item.key} className="mt-0 h-full data-[state=inactive]:hidden">
              {item.type === 'log' && item.logType === 'stdout' && (
                <LogContent content={stdout} searchQuery={searchQuery} isRunning={step.status === 'running'} />
              )}
              {item.type === 'log' && item.logType === 'stderr' && (
                <LogContent content={stderr} searchQuery={searchQuery} isRunning={step.status === 'running'} />
              )}
              {item.type === 'artifact' && item.artifact && getFileExtension(item.artifact.name) === 'md' && (
                <MarkdownViewer artifact={item.artifact} isActive={activeTab === item.key} />
              )}
              {item.type === 'artifact' && item.artifact && getFileExtension(item.artifact.name) !== 'md' && (
                <ArtifactContent artifact={item.artifact} searchQuery={searchQuery} isActive={activeTab === item.key} />
              )}
            </TabsContent>
          ))}
        </div>
      </Tabs>
    </div>
  );
}
