import { useState, useEffect, useMemo } from 'react';
import { getRunDiff, readArtifact, type ArtifactInfo, type CommitDiffInfo } from '@/hooks/use-daemon';
import {
  Loader2,
  FileCode,
  FilePlus,
  FileX,
  FileMinus,
  MessageSquare,
  ChevronDown,
  ChevronRight,
  GitCommitHorizontal,
} from 'lucide-react';
import { cn } from '@/lib/utils';
import { parsePatchFiles, type FileDiffMetadata, type ChangeTypes, type DiffLineAnnotation } from '@pierre/diffs';
import { FileDiff } from '@pierre/diffs/react';
import { ResizablePanelGroup, ResizablePanel, ResizableHandle } from '@airlock-hq/design-system/react';

interface CodeComment {
  file: string;
  line: number;
  message: string;
  severity: 'info' | 'warning' | 'error';
}

interface ChangesTabProps {
  runId: string;
  artifacts: ArtifactInfo[];
}

/**
 * ChangesTab displays file-by-file diffs with inline comments.
 * For multi-commit pushes, files are grouped by commit in the sidebar.
 */
export function ChangesTab({ runId, artifacts }: ChangesTabProps) {
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [patch, setPatch] = useState<string>('');
  const [commits, setCommits] = useState<CommitDiffInfo[]>([]);
  const [comments, setComments] = useState<CodeComment[]>([]);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [selectedCommitSha, setSelectedCommitSha] = useState<string | null>(null);
  const [collapsedCommits, setCollapsedCommits] = useState<Set<string>>(new Set());

  // Load diff and comments
  useEffect(() => {
    async function loadData() {
      setLoading(true);
      setError(null);

      try {
        // Load diff
        const diffResult = await getRunDiff(runId);
        setPatch(diffResult.patch);
        setCommits(diffResult.commits ?? []);

        // Load comments from artifacts
        const commentFiles = artifacts.filter(
          (a) => a.artifact_type === 'file' && a.path.includes('/comments/') && a.path.endsWith('.json')
        );

        const loadedComments: CodeComment[] = [];
        for (const file of commentFiles) {
          try {
            const result = await readArtifact(file.path);
            if (!result.is_binary) {
              const parsed = JSON.parse(result.content);
              if (parsed.comments && Array.isArray(parsed.comments)) {
                loadedComments.push(...parsed.comments);
              }
            }
          } catch (e) {
            console.error('Failed to load comments:', file.path, e);
          }
        }
        setComments(loadedComments);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setLoading(false);
      }
    }

    loadData();
  }, [runId, artifacts]);

  // Parse the overall diff
  const parsedFiles = useMemo(() => {
    if (!patch) return [];
    try {
      const result = parsePatchFiles(patch);
      if (Array.isArray(result) && result.length > 0 && result[0].files) {
        return result[0].files;
      }
      return [];
    } catch (e) {
      console.error('Failed to parse patch:', e);
      return [];
    }
  }, [patch]);

  // Parse per-commit diffs
  const commitParsedFiles = useMemo(() => {
    const map = new Map<string, FileDiffMetadata[]>();
    for (const commit of commits) {
      if (!commit.patch) continue;
      try {
        const result = parsePatchFiles(commit.patch);
        if (Array.isArray(result) && result.length > 0 && result[0].files) {
          map.set(commit.sha, result[0].files);
        }
      } catch (e) {
        console.error('Failed to parse commit patch:', commit.sha, e);
      }
    }
    return map;
  }, [commits]);

  const isMultiCommit = commits.length > 1;

  // Auto-select first file
  useEffect(() => {
    if (parsedFiles.length > 0 && !selectedFile) {
      if (isMultiCommit && commits.length > 0) {
        // Select first file from first commit
        const firstCommitFiles = commitParsedFiles.get(commits[0].sha);
        if (firstCommitFiles && firstCommitFiles.length > 0) {
          setSelectedFile(firstCommitFiles[0].name);
          setSelectedCommitSha(commits[0].sha);
        } else {
          setSelectedFile(parsedFiles[0].name);
        }
      } else {
        setSelectedFile(parsedFiles[0].name);
      }
    }
  }, [parsedFiles, selectedFile, isMultiCommit, commits, commitParsedFiles]);

  // Toggle commit collapse
  const toggleCommit = (sha: string) => {
    setCollapsedCommits((prev) => {
      const next = new Set(prev);
      if (next.has(sha)) {
        next.delete(sha);
      } else {
        next.add(sha);
      }
      return next;
    });
  };

  // Get comments for a specific file
  const getCommentsForFile = (filePath: string) => {
    return comments.filter((c) => c.file === filePath);
  };

  // Calculate additions/deletions for a file from its hunks
  const getFileStats = (file: FileDiffMetadata) => {
    let additions = 0;
    let deletions = 0;
    for (const hunk of file.hunks) {
      additions += hunk.additionCount;
      deletions += hunk.deletionCount;
    }
    return { additions, deletions };
  };

  // Determine which file diff to show: if multi-commit and a commit is selected, use that commit's version
  const selectedFileDiff = useMemo(() => {
    if (isMultiCommit && selectedCommitSha) {
      const commitFiles = commitParsedFiles.get(selectedCommitSha);
      if (commitFiles) {
        const file = commitFiles.find((f) => f.name === selectedFile);
        if (file) return file;
      }
    }
    return parsedFiles.find((f) => f.name === selectedFile) ?? null;
  }, [isMultiCommit, selectedCommitSha, commitParsedFiles, parsedFiles, selectedFile]);

  const selectedFileComments = selectedFile ? getCommentsForFile(selectedFile) : [];

  // Convert CodeComment[] to DiffLineAnnotation[] for @pierre/diffs
  const lineAnnotations: DiffLineAnnotation<CodeComment>[] = selectedFileComments.map((comment) => ({
    side: 'additions' as const,
    lineNumber: comment.line,
    metadata: comment,
  }));

  // Render the PanelGroup from the very first render so react-resizable-panels
  // has time to fully initialise its pointer-event listeners before the user
  // can interact with the handle.  Loading / error / empty states go inside
  // the panels rather than as early returns that defer the PanelGroup mount.
  return (
    <ResizablePanelGroup direction="horizontal" autoSaveId="changes-panels">
      {/* File list sidebar */}
      <ResizablePanel defaultSize={20} minSize={12} maxSize={35}>
        <div className="flex h-full flex-col">
          <div className="border-border-subtle border-b p-3">
            <h3 className="text-small font-medium">
              {loading ? 'Files Changed' : `Files Changed (${parsedFiles.length})`}
            </h3>
          </div>
          <div className="flex-1 overflow-y-auto">
            {loading ? (
              <div className="flex items-center justify-center py-8">
                <Loader2 className="text-foreground-muted h-5 w-5 animate-spin" />
              </div>
            ) : isMultiCommit ? (
              // Multi-commit: collapsible sections per commit
              commits.map((commit) => {
                const isCollapsed = collapsedCommits.has(commit.sha);
                const commitFiles = commitParsedFiles.get(commit.sha) ?? [];
                return (
                  <div key={commit.sha}>
                    {/* Commit header */}
                    <div
                      className="border-border-subtle flex cursor-pointer items-center gap-2 border-b px-3 py-2"
                      onClick={() => toggleCommit(commit.sha)}
                    >
                      {isCollapsed ? (
                        <ChevronRight className="text-foreground-muted h-3.5 w-3.5 shrink-0" />
                      ) : (
                        <ChevronDown className="text-foreground-muted h-3.5 w-3.5 shrink-0" />
                      )}
                      <GitCommitHorizontal className="text-foreground-muted h-3.5 w-3.5 shrink-0" />
                      <div className="min-w-0 flex-1">
                        <div className="text-small flex items-center gap-1.5 truncate font-medium">
                          <span className="text-foreground-muted font-mono text-[11px]">{commit.sha.slice(0, 7)}</span>
                          <span className="truncate">{commit.message}</span>
                        </div>
                      </div>
                      <span className="text-micro text-foreground-muted shrink-0">
                        {commitFiles.length} {commitFiles.length === 1 ? 'file' : 'files'}
                      </span>
                    </div>

                    {/* Files within this commit */}
                    {!isCollapsed &&
                      commitFiles.map((file) => (
                        <FileItem
                          key={`${commit.sha}:${file.name}`}
                          file={file}
                          isSelected={selectedFile === file.name && selectedCommitSha === commit.sha}
                          comments={getCommentsForFile(file.name)}
                          stats={getFileStats(file)}
                          indent
                          onClick={() => {
                            setSelectedFile(file.name);
                            setSelectedCommitSha(commit.sha);
                          }}
                        />
                      ))}
                  </div>
                );
              })
            ) : (
              // Single commit or no commits: flat file list
              parsedFiles.map((file) => (
                <FileItem
                  key={file.name}
                  file={file}
                  isSelected={selectedFile === file.name}
                  comments={getCommentsForFile(file.name)}
                  stats={getFileStats(file)}
                  onClick={() => {
                    setSelectedFile(file.name);
                    setSelectedCommitSha(null);
                  }}
                />
              ))
            )}
          </div>
        </div>
      </ResizablePanel>

      <ResizableHandle />

      {/* Diff viewer */}
      <ResizablePanel defaultSize={80} minSize={50}>
        <div className="h-full overflow-y-auto">
          {loading ? (
            <div className="flex h-full items-center justify-center">
              <Loader2 className="text-foreground-muted h-6 w-6 animate-spin" />
            </div>
          ) : error ? (
            <div className="flex h-full items-center justify-center">
              <p className="text-danger">{error}</p>
            </div>
          ) : parsedFiles.length === 0 ? (
            <div className="flex h-full items-center justify-center">
              <div className="text-center">
                <FileCode className="text-foreground-muted/50 mx-auto mb-4 h-12 w-12" />
                <p className="text-foreground-muted">No changes in this push request</p>
              </div>
            </div>
          ) : selectedFileDiff ? (
            <FileDiff<CodeComment>
              fileDiff={selectedFileDiff}
              options={{
                theme: { dark: 'github-dark', light: 'github-light' },
                themeType: 'light',
                diffStyle: 'unified',
                overflow: 'scroll',
                disableFileHeader: true,
              }}
              lineAnnotations={lineAnnotations}
              renderAnnotation={(annotation) => <CommentAnnotation comment={annotation.metadata} />}
            />
          ) : (
            <div className="flex h-full items-center justify-center">
              <p className="text-foreground-muted">Select a file to view changes</p>
            </div>
          )}
        </div>
      </ResizablePanel>
    </ResizablePanelGroup>
  );
}

// =============================================================================
// FileItem subcomponent
// =============================================================================

interface FileItemProps {
  file: FileDiffMetadata;
  isSelected: boolean;
  comments: CodeComment[];
  stats: { additions: number; deletions: number };
  indent?: boolean;
  onClick: () => void;
}

function FileItem({ file, isSelected, comments, stats, indent, onClick }: FileItemProps) {
  return (
    <div
      className={cn(
        'text-small flex cursor-pointer items-center gap-2 border-l-2 py-2',
        indent ? 'pr-3 pl-7' : 'px-3',
        isSelected ? 'border-l-signal bg-surface/30' : 'hover:bg-surface/20 border-l-transparent'
      )}
      onClick={onClick}
    >
      {getFileIcon(file.type)}
      <span className="min-w-0 flex-1 truncate">{file.name}</span>
      <div className="flex shrink-0 items-center gap-1">
        {comments.length > 0 && (
          <span className="text-micro text-foreground-muted flex items-center">
            <MessageSquare className="mr-0.5 h-3 w-3" />
            {comments.length}
          </span>
        )}
        <span className="text-micro text-success">+{stats.additions}</span>
        <span className="text-micro text-danger">-{stats.deletions}</span>
      </div>
    </div>
  );
}

// =============================================================================
// Helper components
// =============================================================================

interface CommentAnnotationProps {
  comment: CodeComment;
}

function CommentAnnotation({ comment }: CommentAnnotationProps) {
  return (
    <div
      className={cn(
        'text-small my-1 rounded border-l-2 px-3 py-2',
        comment.severity === 'error' && 'border-danger bg-danger/10',
        comment.severity === 'warning' && 'border-warning bg-warning/10',
        comment.severity === 'info' && 'border-signal bg-signal/10'
      )}
    >
      <div className="flex items-center gap-2">
        <MessageSquare className="h-3 w-3" />
        <span className="font-semibold capitalize">{comment.severity}</span>
      </div>
      <p className="mt-1">{comment.message}</p>
    </div>
  );
}

function getFileIcon(type: ChangeTypes) {
  switch (type) {
    case 'new':
      return <FilePlus className="text-success h-4 w-4 shrink-0" />;
    case 'deleted':
      return <FileX className="text-danger h-4 w-4 shrink-0" />;
    case 'rename-pure':
    case 'rename-changed':
      return <FileMinus className="text-warning h-4 w-4 shrink-0" />;
    case 'change':
    default:
      return <FileCode className="text-foreground-muted h-4 w-4 shrink-0" />;
  }
}
