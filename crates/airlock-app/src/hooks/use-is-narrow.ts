import { useState, useEffect, useRef } from 'react';

/**
 * Hook to detect if a container is narrow (for responsive layouts).
 * Uses ResizeObserver to monitor container width changes.
 *
 * @param threshold - Width threshold in pixels (default: 800)
 * @returns Object with containerRef to attach to element and isNarrow boolean
 */
export function useIsNarrow(threshold = 800) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [isNarrow, setIsNarrow] = useState(false);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setIsNarrow(entry.contentRect.width < threshold);
      }
    });

    observer.observe(container);
    return () => observer.disconnect();
  }, [threshold]);

  return { containerRef, isNarrow };
}
