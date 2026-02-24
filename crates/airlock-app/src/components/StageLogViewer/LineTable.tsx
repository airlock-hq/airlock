import { cn } from '@/lib/utils';
import { highlightLine } from './utils';

interface LineTableProps {
  lines: string[];
  searchQuery: string;
  showLineNumbers?: boolean;
  highlightClass?: string;
  /** CSS class for row highlight when search matches a line */
  rowHighlightClass?: string;
}

/** Shared line-rendering table for log and artifact viewers. */
export function LineTable({
  lines,
  searchQuery,
  showLineNumbers = true,
  highlightClass = 'bg-warning/20',
  rowHighlightClass = 'bg-warning/20',
}: LineTableProps) {
  return (
    <table className="w-full border-collapse">
      <tbody>
        {lines.map((line, index) => (
          <tr
            key={index}
            className={cn(
              'hover:bg-terminal-muted',
              searchQuery && line.toLowerCase().includes(searchQuery.toLowerCase()) && rowHighlightClass
            )}
          >
            {showLineNumbers && (
              <td className="border-terminal-border text-terminal-foreground/50 border-r px-2 py-0.5 text-right select-none">
                {index + 1}
              </td>
            )}
            <td className="px-2 py-0.5 break-all whitespace-pre-wrap">
              {highlightLine(line, searchQuery, highlightClass)}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
