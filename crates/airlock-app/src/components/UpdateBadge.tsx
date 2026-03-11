import { useState, useRef, useEffect } from 'react';
import { ArrowUpCircle, Copy, Check } from 'lucide-react';
import { useUpdateCheck } from '@/hooks/use-update-check';

export function UpdateBadge() {
  const { updateAvailable, latestVersion, releaseUrl } = useUpdateCheck();
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const popoverRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function handleClick(e: MouseEvent) {
      if (popoverRef.current && !popoverRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [open]);

  if (!updateAvailable || !latestVersion) return null;

  const command = 'brew update && brew upgrade airlock';

  function handleCopy() {
    navigator.clipboard.writeText(command).then(
      () => {
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
      },
      () => {}
    );
  }

  return (
    <div className="relative" ref={popoverRef}>
      <button
        onClick={() => setOpen(!open)}
        className="text-warning hover:text-warning/80 flex cursor-pointer items-center gap-1.5 transition-colors"
        title="Update available"
      >
        <ArrowUpCircle className="h-4 w-4" />
        <span className="text-small">Update</span>
      </button>

      {open && (
        <div className="bg-background border-border absolute top-full right-0 z-50 mt-2 mr-0 rounded-md border p-3 shadow-lg">
          <p className="text-foreground text-small font-medium">
            Update available:{' '}
            {releaseUrl ? (
              <a href={releaseUrl} target="_blank" rel="noopener noreferrer" className="underline">
                v{latestVersion}
              </a>
            ) : (
              <>v{latestVersion}</>
            )}
          </p>
          <div className="border-border mt-2 inline-flex items-center gap-3 rounded-lg border px-3 py-2">
            <code className="text-foreground text-micro font-mono whitespace-nowrap">{command}</code>
            <button
              onClick={handleCopy}
              className="text-foreground-muted hover:text-foreground cursor-pointer transition-colors"
            >
              {copied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
