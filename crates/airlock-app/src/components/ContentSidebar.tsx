import { FileText, Loader2 } from 'lucide-react';
import { cn } from '@/lib/utils';

export interface ContentItem {
  name: string;
  title: string;
}

interface ContentSidebarProps {
  items: ContentItem[];
  selectedName: string | null;
  onSelect: (name: string) => void;
  loading?: boolean;
}

export function ContentSidebar({ items, selectedName, onSelect, loading }: ContentSidebarProps) {
  return (
    <div className="flex h-full flex-col">
      <div className="border-border-subtle border-b px-4 py-3">
        <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">Content</span>
      </div>
      <div className="flex-1 overflow-y-auto">
        {loading ? (
          <div className="flex items-center justify-center py-8">
            <Loader2 className="text-foreground-muted h-5 w-5 animate-spin" />
          </div>
        ) : (
          items.map((item) => (
            <div
              key={item.name}
              className={cn(
                'cursor-pointer px-4 py-3 transition-colors',
                selectedName === item.name
                  ? 'border-l-signal bg-surface/30 border-l-2'
                  : 'hover:bg-surface/20 border-l-2 border-l-transparent'
              )}
              onClick={() => onSelect(item.name)}
            >
              <div className="flex items-center gap-2.5">
                <FileText className="text-foreground-muted h-4 w-4 shrink-0" />
                <span className="text-small min-w-0 truncate">{item.title}</span>
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
