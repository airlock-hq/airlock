import { useState, useEffect, useRef } from 'react';
import { Badge } from '@airlock-hq/design-system/react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { cn } from '@/lib/utils';
import { LineTable } from './LineTable';

interface LogSectionProps {
  title: string;
  content: string;
  expanded: boolean;
  onToggle: () => void;
  searchQuery: string;
  variant?: 'default' | 'error';
  isRunning?: boolean;
}

export function LogSection({
  title,
  content,
  expanded,
  onToggle,
  searchQuery,
  variant = 'default',
  isRunning,
}: LogSectionProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);

  useEffect(() => {
    if (isRunning && autoScroll && containerRef.current && expanded) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [content, isRunning, autoScroll, expanded]);

  const handleScroll = () => {
    if (!containerRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = containerRef.current;
    const isAtBottom = scrollHeight - scrollTop - clientHeight < 50;
    setAutoScroll(isAtBottom);
  };

  const lines = content.split('\n');
  const lineCount = lines.length;

  return (
    <div className="border-b last:border-b-0">
      <button
        onClick={onToggle}
        className={cn(
          'text-small flex w-full items-center gap-2 px-4 py-2 text-left font-medium transition-colors',
          'hover:bg-surface/50',
          variant === 'error' && 'text-danger'
        )}
      >
        {expanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
        <span>{title}</span>
        <Badge variant="secondary" className="text-micro ml-auto">
          {lineCount} lines
        </Badge>
      </button>

      {expanded && (
        <div
          ref={containerRef}
          onScroll={handleScroll}
          className={cn('text-micro max-h-96 overflow-auto font-mono', 'bg-terminal text-terminal-foreground')}
        >
          <LineTable lines={lines} searchQuery={searchQuery} />
        </div>
      )}
    </div>
  );
}
