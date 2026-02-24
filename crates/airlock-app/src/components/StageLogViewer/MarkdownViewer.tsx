import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { ArtifactInfo } from '@/hooks/use-daemon';
import { Loader2, AlertCircle } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useArtifactLoader } from './hooks';

interface MarkdownViewerProps {
  artifact: ArtifactInfo;
  isActive: boolean;
}

/**
 * MarkdownViewer renders markdown content from an artifact with GFM support.
 * Lazy-loads content when the tab becomes active.
 */
export function MarkdownViewer({ artifact, isActive }: MarkdownViewerProps) {
  const { content, loading, error } = useArtifactLoader(artifact.path, isActive);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-8">
        <Loader2 className="text-foreground-muted h-5 w-5 animate-spin" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="text-danger flex items-center justify-center gap-2 py-8">
        <AlertCircle className="h-4 w-4" />
        <span>{error}</span>
      </div>
    );
  }

  if (!content) {
    return null;
  }

  return (
    <div className="prose prose-sm max-w-none overflow-auto p-4">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          // Style code blocks with terminal background
          pre: ({ children, ...props }) => (
            <pre className={cn('overflow-auto rounded-md p-4', 'bg-terminal text-terminal-foreground')} {...props}>
              {children}
            </pre>
          ),
          // Style inline code
          code: ({ children, className, ...props }) => {
            // Check if this is a code block (has language class) vs inline code
            const isBlock = className?.includes('language-');
            if (isBlock) {
              return (
                <code className={className} {...props}>
                  {children}
                </code>
              );
            }
            return (
              <code className="bg-surface text-small rounded px-1.5 py-0.5" {...props}>
                {children}
              </code>
            );
          },
          // Style tables
          table: ({ children, ...props }) => (
            <table className="border-border w-full border-collapse border" {...props}>
              {children}
            </table>
          ),
          th: ({ children, ...props }) => (
            <th className="border-border bg-surface border px-3 py-2 text-left font-semibold" {...props}>
              {children}
            </th>
          ),
          td: ({ children, ...props }) => (
            <td className="border-border border px-3 py-2" {...props}>
              {children}
            </td>
          ),
          // Style task lists
          input: ({ ...props }) => <input className="mr-2" disabled {...props} />,
          // Style links
          a: ({ children, href, ...props }) => (
            <a href={href} className="text-signal hover:underline" target="_blank" rel="noopener noreferrer" {...props}>
              {children}
            </a>
          ),
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}
