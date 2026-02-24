import { useState, useEffect, useMemo, useCallback } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { readArtifact, type ArtifactInfo } from '@/hooks/use-daemon';
import { Loader2, FileText } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useSearchParams } from 'react-router-dom';
import { ContentSidebar } from '@/components/ContentSidebar';
import { ResizablePanelGroup, ResizablePanel, ResizableHandle } from '@airlock-hq/design-system/react';

interface ContentArtifact {
  name: string;
  title: string;
  content: string;
  created_at: number;
}

interface ContentTabProps {
  artifacts: ArtifactInfo[];
}

export function ContentTab({ artifacts }: ContentTabProps) {
  const [contentArtifacts, setContentArtifacts] = useState<ContentArtifact[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchParams, setSearchParams] = useSearchParams();

  const contentFiles = useMemo(
    () => artifacts.filter((a) => a.artifact_type === 'file' && a.path.includes('/content/') && a.path.endsWith('.md')),
    [artifacts]
  );

  useEffect(() => {
    async function loadContentArtifacts() {
      if (contentFiles.length === 0) {
        setLoading(false);
        return;
      }

      setLoading(true);
      const loaded: ContentArtifact[] = [];

      for (const file of contentFiles) {
        try {
          const result = await readArtifact(file.path);
          if (!result.is_binary) {
            const { title, content } = parseContentWithFrontmatter(result.content, file.name);
            loaded.push({ name: file.name, title, content, created_at: file.created_at });
          }
        } catch (e) {
          console.error('Failed to load content artifact:', file.path, e);
        }
      }

      loaded.sort((a, b) => a.created_at - b.created_at);
      setContentArtifacts(loaded);
      setLoading(false);
    }

    loadContentArtifacts();
  }, [contentFiles]);

  const contentParam = searchParams.get('content');

  const selectedName = useMemo(() => {
    if (contentParam && contentArtifacts.find((a) => a.name === contentParam)) {
      return contentParam;
    }
    if (contentArtifacts.length > 0) {
      return contentArtifacts[0].name;
    }
    return null;
  }, [contentParam, contentArtifacts]);

  const handleSelect = useCallback(
    (name: string) => {
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          next.set('content', name);
          return next;
        },
        { replace: true }
      );
    },
    [setSearchParams]
  );

  const selectedArtifact = useMemo(
    () => contentArtifacts.find((a) => a.name === selectedName) ?? null,
    [contentArtifacts, selectedName]
  );

  // Render the PanelGroup from the very first render so react-resizable-panels
  // has time to fully initialise its pointer-event listeners before the user
  // can interact with the handle.  Loading / empty states go inside the panels
  // rather than as early returns that defer the PanelGroup mount.
  return (
    <ResizablePanelGroup direction="horizontal" autoSaveId="content-panels">
      <ResizablePanel defaultSize={20} minSize={12} maxSize={35}>
        <ContentSidebar
          items={contentArtifacts.map((a) => ({ name: a.name, title: a.title }))}
          selectedName={selectedName}
          onSelect={handleSelect}
          loading={loading}
        />
      </ResizablePanel>

      <ResizableHandle />

      <ResizablePanel defaultSize={80} minSize={50}>
        <div className="h-full overflow-y-auto">
          {loading ? (
            <div className="flex h-full items-center justify-center">
              <Loader2 className="text-foreground-muted h-6 w-6 animate-spin" />
            </div>
          ) : contentArtifacts.length === 0 ? (
            <div className="flex h-full items-center justify-center">
              <div className="flex flex-col items-center justify-center py-12 text-center">
                <FileText className="text-foreground-muted/50 mb-4 h-12 w-12" />
                <p className="text-foreground-muted">No content artifacts generated yet.</p>
                <p className="text-small text-foreground-muted/75 mt-1">
                  Stages can create content using{' '}
                  <code className="bg-surface rounded px-1">airlock artifact content</code>
                </p>
              </div>
            </div>
          ) : selectedArtifact ? (
            <div className="p-8">
              <ContentCard artifact={selectedArtifact} />
            </div>
          ) : (
            <div className="flex h-full items-center justify-center">
              <p className="text-foreground-muted">Select a content item</p>
            </div>
          )}
        </div>
      </ResizablePanel>
    </ResizablePanelGroup>
  );
}

function ContentCard({ artifact }: { artifact: ContentArtifact }) {
  return (
    <div className="prose prose-sm max-w-none">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          pre: ({ children, ...props }) => (
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
          ),
          code: ({ children, className, ...props }) => (
            <code className={cn('bg-surface text-small rounded px-1.5 py-0.5', className)} {...props}>
              {children}
            </code>
          ),
        }}
      >
        {artifact.content}
      </ReactMarkdown>
    </div>
  );
}

function parseContentWithFrontmatter(content: string, defaultTitle: string): { title: string; content: string } {
  const frontmatterRegex = /^---\n([\s\S]*?)\n---\n/;
  const match = content.match(frontmatterRegex);

  if (!match) {
    return { title: defaultTitle, content };
  }

  const frontmatter = match[1];
  const body = content.slice(match[0].length);
  const titleMatch = frontmatter.match(/^title:\s*"?(.+?)"?$/m);
  const title = titleMatch ? titleMatch[1].trim() : defaultTitle;

  return { title, content: body };
}
