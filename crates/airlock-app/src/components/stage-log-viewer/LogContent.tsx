import { useState, useEffect, useRef } from 'react';
import { cn } from '@/lib/utils';
import { LineTable } from './LineTable';

interface LogContentProps {
  content: string;
  searchQuery: string;
  variant?: 'default' | 'error';
  isRunning?: boolean;
}

/**
 * LogContent displays log lines with search highlighting.
 * Supports auto-scroll for running stages.
 */
export function LogContent({ content, searchQuery, isRunning }: LogContentProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);

  useEffect(() => {
    if (isRunning && autoScroll && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [content, isRunning, autoScroll]);

  const handleScroll = () => {
    if (!containerRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = containerRef.current;
    const isAtBottom = scrollHeight - scrollTop - clientHeight < 50;
    setAutoScroll(isAtBottom);
  };

  const lines = content.split('\n');

  return (
    <div
      ref={containerRef}
      onScroll={handleScroll}
      className={cn('text-micro h-full overflow-auto font-mono', 'bg-terminal text-terminal-foreground')}
    >
      <LineTable
        lines={lines}
        searchQuery={searchQuery}
        showLineNumbers={false}
        highlightClass="bg-signal"
        rowHighlightClass="bg-signal/20"
      />
    </div>
  );
}
