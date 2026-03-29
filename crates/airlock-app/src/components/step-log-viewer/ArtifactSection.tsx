import { useState } from 'react';
import { Badge } from '@airlock-hq/design-system/react';
import type { ArtifactInfo } from '@/hooks/use-daemon';
import { ChevronDown, ChevronRight, Loader2 } from 'lucide-react';
import { cn } from '@/lib/utils';
import { ArtifactIcon } from './ArtifactIcon';
import { getArtifactTypeName } from './utils';
import { useArtifactLoader } from './hooks';
import { LineTable } from './LineTable';

interface ArtifactSectionProps {
  artifact: ArtifactInfo;
  searchQuery: string;
}

export function ArtifactSection({ artifact, searchQuery }: ArtifactSectionProps) {
  const [expanded, setExpanded] = useState(true);
  const { content, loading, error } = useArtifactLoader(artifact.path, expanded);

  const lines = content?.split('\n') ?? [];
  const lineCount = lines.length;

  return (
    <div className="border-b last:border-b-0">
      <button
        onClick={() => setExpanded(!expanded)}
        className="text-small hover:bg-surface/50 flex w-full items-center gap-2 px-4 py-2 text-left font-medium transition-colors"
      >
        {expanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
        <ArtifactIcon artifactType={artifact.artifact_type} className="text-foreground-muted h-4 w-4" />
        <span>{artifact.name}</span>
        <span className="text-micro text-foreground-muted">({getArtifactTypeName(artifact.artifact_type)})</span>
        {content && (
          <Badge variant="secondary" className="text-micro ml-auto">
            {lineCount} lines
          </Badge>
        )}
        {loading && <Loader2 className="ml-auto h-3 w-3 animate-spin" />}
      </button>

      {expanded && (
        <div className={cn('text-micro max-h-96 overflow-auto font-mono', 'bg-terminal text-terminal-foreground')}>
          {loading && (
            <div className="flex items-center justify-center py-8">
              <Loader2 className="text-foreground-muted h-5 w-5 animate-spin" />
            </div>
          )}
          {error && (
            <div className="text-danger flex items-center justify-center gap-2 py-8">
              <span>{error}</span>
            </div>
          )}
          {content && !loading && !error && <LineTable lines={lines} searchQuery={searchQuery} />}
        </div>
      )}
    </div>
  );
}
