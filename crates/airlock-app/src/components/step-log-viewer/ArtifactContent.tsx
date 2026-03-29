import type { ArtifactInfo } from '@/hooks/use-daemon';
import { Loader2, AlertCircle } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useArtifactLoader } from './hooks';
import { LineTable } from './LineTable';

interface ArtifactContentProps {
  artifact: ArtifactInfo;
  searchQuery: string;
  isActive: boolean;
}

/**
 * ArtifactContent displays artifact file content with line numbers and search highlighting.
 * Lazy-loads content when the tab becomes active.
 */
export function ArtifactContent({ artifact, searchQuery, isActive }: ArtifactContentProps) {
  const { content, loading, error } = useArtifactLoader(artifact.path, isActive);

  const lines = content?.split('\n') ?? [];

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
    <div className={cn('text-micro h-full overflow-auto font-mono', 'bg-terminal text-terminal-foreground')}>
      <LineTable lines={lines} searchQuery={searchQuery} />
    </div>
  );
}
