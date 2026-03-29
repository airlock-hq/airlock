import { useState, useEffect } from 'react';
import { readArtifact } from '@/hooks/use-daemon';

/** Shared hook for lazy-loading artifact content when a trigger becomes true. */
export function useArtifactLoader(artifactPath: string, trigger: boolean) {
  const [content, setContent] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!trigger || content !== null) return;

    let cancelled = false;

    const fetchArtifact = async () => {
      setLoading(true);
      try {
        const result = await readArtifact(artifactPath);
        if (!cancelled) {
          setContent(result.content);
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load artifact');
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    };

    fetchArtifact();

    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [trigger, artifactPath]);

  return { content, loading, error };
}
