import { useState } from 'react';
import { FileCode, FileText, MessageSquare, ChevronDown, ChevronUp, Loader2, CheckCircle2, Check } from 'lucide-react';
import { CritiqueComment, ExpandableCard } from '@airlock-hq/design-system/react';
import { cn } from '@/lib/utils';
import { useArtifactLoader } from '@/components/stage-log-viewer/hooks';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { MermaidDiagram } from '@/components/MermaidDiagram';
import type { FeedEvent, ArtifactCategory } from './types';
import { getCommentKey, getPatchId, type CodeComment } from '@/lib/artifact-keys';

type ArtifactEvent = Extract<FeedEvent, { type: 'artifact' }>;

interface ArtifactItemProps {
  event: ArtifactEvent;
  selectedComments: Set<string>;
  onToggleComment: (key: string) => void;
  selectedPatches: Set<string>;
  onTogglePatch: (id: string) => void;
}

function getCategoryIcon(category: ArtifactCategory) {
  switch (category) {
    case 'patch':
      return <FileCode className="text-foreground-muted h-4 w-4 shrink-0" />;
    case 'comment':
      return <MessageSquare className="text-foreground-muted h-4 w-4 shrink-0" />;
    case 'content':
      return <FileText className="text-foreground-muted h-4 w-4 shrink-0" />;
  }
}

function getTitle(artifact: ArtifactEvent['artifact']): string {
  return artifact.name.replace(/\.(json|md)$/, '').replace(/[-_]/g, ' ');
}

export function ArtifactItem({
  event,
  selectedComments,
  onToggleComment,
  selectedPatches,
  onTogglePatch,
}: ArtifactItemProps) {
  const { category } = event;

  switch (category) {
    case 'patch':
      return <PatchArtifactItem event={event} selectedPatches={selectedPatches} onTogglePatch={onTogglePatch} />;
    case 'comment':
      return (
        <CommentArtifactItem event={event} selectedComments={selectedComments} onToggleComment={onToggleComment} />
      );
    case 'content':
      return <ContentArtifactItem event={event} />;
  }
}

// ---------------------------------------------------------------------------
// Patch artifact — shows title + diff stats, expandable diff
// ---------------------------------------------------------------------------

function PatchArtifactItem({
  event,
  selectedPatches,
  onTogglePatch,
}: {
  event: ArtifactEvent;
  selectedPatches: Set<string>;
  onTogglePatch: (id: string) => void;
}) {
  const isApplied = event.artifact.path.includes('/patches/applied/');
  const patchId = getPatchId(event.artifact.path);
  const isSelected = selectedPatches.has(patchId);
  const [expanded, setExpanded] = useState(!isApplied);
  const { content, loading } = useArtifactLoader(event.artifact.path, true);
  const parsed = content ? safeParse<{ title?: string; explanation?: string; diff?: string }>(content) : null;
  const title = parsed?.title || getTitle(event.artifact);
  const explanation = parsed?.explanation;
  const diff = parsed?.diff ?? '';

  return (
    <div>
      <div
        className="hover:bg-surface/40 flex cursor-pointer items-center gap-3 px-4 py-2"
        onClick={() => setExpanded(!expanded)}
      >
        {isApplied ? (
          <CheckCircle2 className="text-success h-4 w-4 shrink-0" />
        ) : (
          <div
            className={cn(
              'flex h-4 w-4 shrink-0 cursor-pointer items-center justify-center rounded border',
              isSelected ? 'border-signal bg-signal text-background' : 'border-foreground-muted'
            )}
            onClick={(e) => {
              e.stopPropagation();
              onTogglePatch(patchId);
            }}
          >
            {isSelected && <Check className="h-2.5 w-2.5" />}
          </div>
        )}
        {getCategoryIcon('patch')}
        <span className="text-small min-w-0 flex-1 truncate py-1">{title}</span>
        {explanation && (
          <span className="text-small text-foreground-muted hidden truncate sm:inline">{explanation}</span>
        )}
        {diff && <PatchStats diff={diff} />}
        {expanded ? (
          <ChevronUp className="text-foreground-muted h-3.5 w-3.5 shrink-0" />
        ) : (
          <ChevronDown className="text-foreground-muted h-3.5 w-3.5 shrink-0" />
        )}
      </div>

      {expanded && (
        <div className="border-border-subtle mx-4 mb-4 overflow-hidden rounded border">
          {loading ? (
            <div className="flex items-center justify-center py-4">
              <Loader2 className="text-foreground-muted h-4 w-4 animate-spin" />
            </div>
          ) : diff ? (
            <ExpandableCard>
              <DiffPreview diff={diff} />
            </ExpandableCard>
          ) : (
            <div className="text-small text-foreground-muted p-4">No diff content</div>
          )}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Comment artifact — inline severity-colored cards
// ---------------------------------------------------------------------------

function CommentArtifactItem({
  event,
  selectedComments,
  onToggleComment,
}: {
  event: ArtifactEvent;
  selectedComments: Set<string>;
  onToggleComment: (key: string) => void;
}) {
  const [expanded, setExpanded] = useState(true);
  const { content, loading } = useArtifactLoader(event.artifact.path, true);

  const parsed = content ? safeParse<{ comments?: CodeComment[] }>(content) : null;
  const comments = parsed?.comments ?? [];

  const commentKeys = comments.map(getCommentKey);
  const selectedCount = commentKeys.filter((k) => selectedComments.has(k)).length;
  const allSelected = comments.length > 0 && selectedCount === comments.length;

  const handleToggleAll = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (allSelected) {
      for (const key of commentKeys) onToggleComment(key);
    } else {
      for (const key of commentKeys) {
        if (!selectedComments.has(key)) onToggleComment(key);
      }
    }
  };

  return (
    <div>
      <div
        className="hover:bg-surface/40 flex cursor-pointer items-center gap-3 px-4 py-2"
        onClick={() => setExpanded(!expanded)}
      >
        {comments.length > 0 ? (
          <div
            className={cn(
              'flex h-4 w-4 shrink-0 cursor-pointer items-center justify-center rounded border',
              allSelected
                ? 'border-signal bg-signal text-background'
                : selectedCount > 0
                  ? 'border-signal bg-signal/20'
                  : 'border-foreground-muted'
            )}
            onClick={handleToggleAll}
          >
            {allSelected && <Check className="h-2.5 w-2.5" />}
            {!allSelected && selectedCount > 0 && <div className="bg-signal h-1.5 w-1.5 rounded-sm" />}
          </div>
        ) : (
          <CheckCircle2 className="text-success h-4 w-4 shrink-0" />
        )}
        {getCategoryIcon('comment')}
        <span className="text-small min-w-0 flex-1 truncate py-1">
          Critique comments
          {comments.length > 0 && <span className="text-foreground-muted ml-1">({comments.length})</span>}
        </span>
        {expanded ? (
          <ChevronUp className="text-foreground-muted h-3.5 w-3.5 shrink-0" />
        ) : (
          <ChevronDown className="text-foreground-muted h-3.5 w-3.5 shrink-0" />
        )}
      </div>

      {expanded && !loading && comments.length > 0 && (
        <div className="mx-4 mb-4 space-y-2">
          {comments.map((comment, i) => {
            const commentKey = getCommentKey(comment);
            return (
              <CritiqueComment
                key={i}
                severity={comment.severity}
                message={comment.message}
                file={comment.file}
                line={comment.line}
                selected={selectedComments.has(commentKey)}
                onToggle={() => onToggleComment(commentKey)}
              />
            );
          })}
        </div>
      )}

      {expanded && loading && (
        <div className="flex items-center justify-center py-3">
          <Loader2 className="text-foreground-muted h-4 w-4 animate-spin" />
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Content artifact — markdown auto-expanded, shows title from frontmatter
// ---------------------------------------------------------------------------

function ContentArtifactItem({ event }: { event: ArtifactEvent }) {
  const isMarkdown = event.artifact.name.endsWith('.md');
  const [expanded, setExpanded] = useState(isMarkdown);
  // Always trigger loading for markdown (auto-expanded) so we can extract the title
  const { content, loading } = useArtifactLoader(event.artifact.path, isMarkdown || expanded);

  const title = content ? parseFrontmatterTitle(content) || getTitle(event.artifact) : getTitle(event.artifact);

  return (
    <div>
      <div
        className="hover:bg-surface/40 flex cursor-pointer items-center gap-3 px-4 py-2"
        onClick={() => setExpanded(!expanded)}
      >
        <CheckCircle2 className="text-success h-4 w-4 shrink-0" />
        {getCategoryIcon('content')}
        <span className="text-small min-w-0 flex-1 truncate py-1">{title}</span>
        {expanded ? (
          <ChevronUp className="text-foreground-muted h-3.5 w-3.5 shrink-0" />
        ) : (
          <ChevronDown className="text-foreground-muted h-3.5 w-3.5 shrink-0" />
        )}
      </div>

      {expanded && (
        <div className="border-border-subtle mx-4 mb-4 overflow-hidden rounded border">
          {loading ? (
            <div className="flex items-center justify-center py-4">
              <Loader2 className="text-foreground-muted h-4 w-4 animate-spin" />
            </div>
          ) : content ? (
            <ExpandableCard>
              <div className="prose prose-sm max-w-none p-4">
                <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
                  {stripFrontmatter(content)}
                </ReactMarkdown>
              </div>
            </ExpandableCard>
          ) : (
            <div className="text-small text-foreground-muted p-4">No content</div>
          )}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

import type { Components } from 'react-markdown';

const markdownComponents: Components = {
  pre: ({ children, ...props }) => {
    const child = Array.isArray(children) ? children[0] : children;
    if (child && typeof child === 'object' && 'type' in child && child.type === MermaidDiagram) {
      return <>{children}</>;
    }
    return (
      <pre
        className={cn(
          'border-border-subtle bg-surface-elevated overflow-auto rounded-md border p-4',
          'text-foreground text-small font-mono',
          '[&_code]:bg-transparent [&_code]:p-0 [&_code]:text-[length:inherit] [&_code]:text-inherit'
        )}
        {...props}
      >
        {children}
      </pre>
    );
  },
  code: ({ children, className, ...props }) => {
    if (className?.includes('language-mermaid')) {
      const chart = String(children).replace(/\n$/, '');
      return <MermaidDiagram chart={chart} />;
    }
    return (
      <code className={cn('bg-surface text-small rounded px-1.5 py-0.5', className)} {...props}>
        {children}
      </code>
    );
  },
};

function safeParse<T>(json: string): T | null {
  try {
    return JSON.parse(json) as T;
  } catch {
    return null;
  }
}

function stripFrontmatter(content: string): string {
  const match = content.match(/^---\n[\s\S]*?\n---\n/);
  return match ? content.slice(match[0].length) : content;
}

function parseFrontmatterTitle(content: string): string | null {
  const match = content.match(/^---\n([\s\S]*?)\n---\n/);
  if (!match) return null;
  const titleMatch = match[1].match(/^title:\s*"?(.+?)"?$/m);
  return titleMatch ? titleMatch[1].trim() : null;
}

function PatchStats({ diff }: { diff: string }) {
  const lines = diff.split('\n');
  let additions = 0;
  let deletions = 0;

  for (const line of lines) {
    if (line.startsWith('+') && !line.startsWith('+++')) additions++;
    else if (line.startsWith('-') && !line.startsWith('---')) deletions++;
  }

  return (
    <span className="text-small flex shrink-0 items-center gap-2 font-mono">
      <span className="text-success">+{additions}</span>
      <span className="text-danger">-{deletions}</span>
    </span>
  );
}

function DiffPreview({ diff }: { diff: string }) {
  const lines = diff.split('\n');

  return (
    <div className="text-micro font-mono">
      {lines.map((line, index) => {
        let className = 'px-4 py-0.5';
        if (line.startsWith('+') && !line.startsWith('+++')) {
          className += ' bg-success/10 text-success';
        } else if (line.startsWith('-') && !line.startsWith('---')) {
          className += ' bg-danger/10 text-danger';
        } else if (line.startsWith('@@')) {
          className += ' bg-signal/10 text-signal';
        } else if (line.startsWith('diff ') || line.startsWith('index ')) {
          className += ' text-foreground-muted';
        } else {
          className += ' text-foreground';
        }

        return (
          <div key={index} className={className}>
            <pre className="whitespace-pre-wrap">{line || ' '}</pre>
          </div>
        );
      })}
    </div>
  );
}
