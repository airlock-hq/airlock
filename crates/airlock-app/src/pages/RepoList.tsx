import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@airlock-hq/design-system/react';
import { Badge, StatusDot } from '@airlock-hq/design-system/react';
import { Button } from '@airlock-hq/design-system/react';
import { useRepos, syncRepo } from '@/hooks/use-daemon';
import { GitBranch, RefreshCw, ExternalLink, Clock, FolderOpen } from 'lucide-react';
import { Link } from 'react-router-dom';
import { useState } from 'react';

export function RepoList() {
  const { repos, loading, error, refresh } = useRepos();
  const [syncing, setSyncing] = useState<string | null>(null);

  const handleSync = async (repoId: string) => {
    try {
      setSyncing(repoId);
      await syncRepo(repoId);
      await refresh();
    } catch (e) {
      console.error('Sync failed:', e);
    } finally {
      setSyncing(null);
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-end justify-between">
        <div>
          <h1 className="text-h1 text-foreground font-bold tracking-tight">Repositories</h1>
          <p className="text-small text-foreground-muted mt-1">Manage your Airlock-enrolled repositories</p>
        </div>
        <Button
          variant="outline"
          className="border-border-subtle bg-background/60"
          onClick={refresh}
          disabled={loading}
        >
          <RefreshCw className={`mr-2 h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
          Refresh
        </Button>
      </div>

      {error && (
        <div className="border-danger/30 bg-danger/5 flex items-center gap-3 rounded-lg border px-4 py-3">
          <StatusDot variant="danger" size="md" />
          <span className="text-small text-danger">{error}</span>
        </div>
      )}

      {loading ? (
        <div className="border-border-subtle bg-background/60 flex flex-col items-center gap-6 rounded-lg border py-20">
          <p className="text-foreground-muted text-center">Loading repositories...</p>
        </div>
      ) : repos.length === 0 ? (
        <div className="border-border-subtle bg-background/60 flex flex-col items-center gap-6 rounded-lg border py-20">
          <GitBranch className="text-foreground-muted/30 h-12 w-12" />
          <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">
            NO REPOSITORIES ENROLLED
          </span>
          <p className="text-small text-foreground-muted mx-auto max-w-md text-center">
            Airlock intercepts your git pushes and transforms them into clean, reviewable changes.
          </p>
          <div className="bg-surface/60 rounded-md px-6 py-4">
            <p className="text-micro text-foreground-muted mb-2 font-mono tracking-widest uppercase">QUICK START</p>
            <code className="text-micro text-foreground block font-mono">
              cd your-project
              <br />
              airlock init
            </code>
          </div>
        </div>
      ) : (
        <div className="grid gap-4">
          {repos.map((repo) => (
            <Card key={repo.id} className="border-border-subtle bg-background/60 transition-colors">
              <CardHeader className="pb-3">
                <div className="flex items-start justify-between">
                  <div className="flex items-center gap-3">
                    <GitBranch className="text-signal h-5 w-5" />
                    <CardTitle className="text-body">{getRepoName(repo.upstream_url)}</CardTitle>
                  </div>
                  <div className="flex items-center gap-2">
                    {repo.pending_runs > 0 && <Badge variant="warning">{repo.pending_runs} pending</Badge>}
                    <Badge variant={repo.last_sync ? 'success' : 'secondary'}>
                      {repo.last_sync ? 'Synced' : 'Never synced'}
                    </Badge>
                  </div>
                </div>
                <CardDescription className="text-micro mt-2 flex items-center gap-1.5 font-mono">
                  <ExternalLink className="h-3 w-3" />
                  {repo.upstream_url}
                </CardDescription>
              </CardHeader>
              <CardContent>
                <div className="flex items-center justify-between">
                  <div className="text-small text-foreground-muted flex items-center gap-6">
                    <div className="flex items-center gap-1.5">
                      <FolderOpen className="h-3.5 w-3.5" />
                      <span className="text-micro max-w-[240px] truncate font-mono">{repo.working_path}</span>
                    </div>
                    {repo.last_sync && (
                      <div className="flex items-center gap-1.5">
                        <Clock className="h-3.5 w-3.5" />
                        <span className="text-micro font-mono">{formatTime(repo.last_sync)}</span>
                      </div>
                    )}
                  </div>
                  <div className="flex items-center gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      className="border-border-subtle text-micro h-8"
                      onClick={() => handleSync(repo.id)}
                      disabled={syncing === repo.id}
                    >
                      <RefreshCw className={`mr-1.5 h-3.5 w-3.5 ${syncing === repo.id ? 'animate-spin' : ''}`} />
                      Sync
                    </Button>
                    <Button asChild size="sm" className="text-micro h-8">
                      <Link to={`/runs?filter=${encodeURIComponent(getRepoName(repo.upstream_url))}`}>View Runs</Link>
                    </Button>
                  </div>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}

// Helper to extract repo name from URL
function getRepoName(url: string): string {
  const sshMatch = url.match(/[:/]([^/]+\/[^/.]+)(\.git)?$/);
  if (sshMatch) return sshMatch[1];

  const httpsMatch = url.match(/\/([^/]+\/[^/.]+)(\.git)?$/);
  if (httpsMatch) return httpsMatch[1];

  return url;
}

// Helper to format timestamp
function formatTime(timestamp: number): string {
  const date = new Date(timestamp * 1000);
  const now = new Date();
  const diff = now.getTime() - date.getTime();

  if (diff < 60000) return 'Just now';
  if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`;
  if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`;
  return date.toLocaleDateString();
}
