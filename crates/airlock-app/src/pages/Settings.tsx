import { useState, useEffect, useCallback } from 'react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@airlock-hq/design-system/react';
import { Button, StatusDot } from '@airlock-hq/design-system/react';
import { Badge } from '@airlock-hq/design-system/react';
import { useDaemonHealth, useRepos, getConfig, type RepoConfigInfo, type GetConfigResult } from '@/hooks/use-daemon';
import {
  Server,
  Database,
  Folder,
  RefreshCw,
  AlertCircle,
  GitBranch,
  ChevronDown,
  ChevronRight,
  FileText,
  Bot,
} from 'lucide-react';

// Component for editing a single repo's policies
function RepoPolicyCard({ repoId, workingPath }: { repoId: string; workingPath: string }) {
  const [expanded, setExpanded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [repoConfig, setRepoConfig] = useState<RepoConfigInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Load repo config when expanded
  const loadRepoConfig = useCallback(async () => {
    if (!expanded) return;
    try {
      setLoading(true);
      const result = await getConfig(repoId);
      if (result.repo) {
        setRepoConfig(result.repo);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [repoId, expanded]);

  useEffect(() => {
    loadRepoConfig();
  }, [loadRepoConfig]);

  // Get shortened path for display
  const shortPath = workingPath.split('/').slice(-2).join('/');

  return (
    <div className="border-border-subtle bg-background/40 rounded-lg border">
      <button
        type="button"
        className="hover:bg-surface/30 flex w-full items-center justify-between p-4 transition-colors"
        onClick={() => setExpanded(!expanded)}
      >
        <div className="flex items-center gap-2 text-left">
          {expanded ? <ChevronDown className="h-4 w-4 shrink-0" /> : <ChevronRight className="h-4 w-4 shrink-0" />}
          <span className="truncate font-medium">{shortPath}</span>
          {repoConfig?.config_exists && (
            <Badge variant="secondary" className="text-micro">
              Custom
            </Badge>
          )}
        </div>
      </button>

      {expanded && (
        <div className="border-border-subtle space-y-5 border-t p-4">
          {loading ? (
            <div className="text-foreground-muted flex items-center gap-2 py-2">
              <RefreshCw className="h-4 w-4 animate-spin" />
              Loading configuration...
            </div>
          ) : error ? (
            <div className="text-danger flex items-center gap-2 py-2">
              <AlertCircle className="h-4 w-4" />
              {error}
            </div>
          ) : (
            <div className="space-y-4">
              {/* Repo info */}
              <div className="text-micro text-foreground-muted">
                <p className="truncate font-mono">{workingPath}</p>
                {repoConfig?.config_path && (
                  <p className="mt-1">
                    Config: <span className="font-mono">{repoConfig.config_path}</span>
                  </p>
                )}
              </div>

              {/* Workflows Section */}
              <div>
                <span className="text-micro text-foreground-muted mb-3 block font-mono tracking-widest uppercase">
                  Workflows
                </span>

                {repoConfig?.workflows.length === 0 ? (
                  <div className="text-small text-foreground-muted">
                    <p>No workflow files found.</p>
                    <p className="mt-1">
                      Add workflow files to <code className="bg-surface rounded px-1">.airlock/workflows/</code> to
                      configure pipelines.
                    </p>
                  </div>
                ) : (
                  <div className="space-y-2">
                    {repoConfig?.workflows.map((wf) => (
                      <div key={wf.filename} className="flex items-center gap-2">
                        <FileText className="text-foreground-muted h-4 w-4 shrink-0" />
                        <code className="bg-surface text-small rounded px-1">{wf.filename}</code>
                        {wf.name && <span className="text-small text-foreground-muted">— {wf.name}</span>}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export function Settings() {
  const { health } = useDaemonHealth();
  const { repos, loading: reposLoading, error: reposError, refresh: refreshRepos } = useRepos();

  // Global config state
  const [globalConfig, setGlobalConfig] = useState<GetConfigResult | null>(null);
  const [configLoading, setConfigLoading] = useState(true);
  const [configError, setConfigError] = useState<string | null>(null);

  const refreshConfig = useCallback(async () => {
    try {
      setConfigLoading(true);
      const result = await getConfig();
      setGlobalConfig(result);
      setConfigError(null);
    } catch (e) {
      setConfigError(e instanceof Error ? e.message : String(e));
    } finally {
      setConfigLoading(false);
    }
  }, []);

  useEffect(() => {
    refreshConfig();
  }, [refreshConfig]);

  const config = globalConfig;

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-h1 text-foreground font-bold tracking-tight">Settings</h1>
        <p className="text-small text-foreground-muted mt-1">Configure Airlock preferences</p>
      </div>

      {configError && (
        <div className="border-danger bg-danger/10 text-small text-danger flex items-center gap-2 rounded-md border p-3">
          <AlertCircle className="h-4 w-4" />
          Failed to load configuration: {configError}
        </div>
      )}

      <div className="grid gap-5">
        {/* File Locations Card */}
        <Card className="border-border-subtle bg-background/60">
          <CardHeader>
            <CardTitle className="flex items-center gap-2.5">
              <Folder className="text-foreground-muted h-5 w-5" />
              File Locations
            </CardTitle>
            <CardDescription>Where Airlock stores its data</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="grid grid-cols-2 gap-x-12 gap-y-4">
              {[
                { label: 'CONFIGURATION', path: config?.global.config_path ?? '~/.airlock/config.toml' },
                { label: 'DATABASE', path: '~/.airlock/state.sqlite' },
                { label: 'GATE REPOS', path: '~/.airlock/repos/' },
                { label: 'ARTIFACTS', path: '~/.airlock/artifacts/' },
              ].map((item) => (
                <div key={item.label}>
                  <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">
                    {item.label}
                  </span>
                  <p className="text-micro text-foreground mt-1 font-mono">{item.path}</p>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>

        {/* Agent Card */}
        <Card className={`border-border-subtle bg-background/60 ${configLoading ? 'opacity-50' : ''}`}>
          <CardHeader>
            <CardTitle className="flex items-center gap-2.5">
              <Bot className="text-foreground-muted h-5 w-5" />
              Agent
            </CardTitle>
            <CardDescription>AI agent used for pipeline steps</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="grid grid-cols-3 gap-8">
              <div>
                <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">Adapter</span>
                <div className="mt-1">
                  <Badge variant="secondary">{config?.global.agent?.adapter ?? 'Auto-detect'}</Badge>
                </div>
              </div>
              <div>
                <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">Model</span>
                <p className="text-small mt-1 font-mono">{config?.global.agent?.model ?? 'Default'}</p>
              </div>
              <div>
                <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">Max Turns</span>
                <p className="text-small mt-1 font-mono">{config?.global.agent?.max_turns ?? 'Default'}</p>
              </div>
            </div>
          </CardContent>
        </Card>

        {/* Daemon Information Card */}
        <Card className={`border-border-subtle bg-background/60 ${configLoading ? 'opacity-50' : ''}`}>
          <CardHeader>
            <CardTitle className="flex items-center gap-2.5">
              <Server className="text-foreground-muted h-5 w-5" />
              Daemon Information
            </CardTitle>
            <CardDescription>Current Airlock daemon status</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="grid grid-cols-2 gap-8">
              <div>
                <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">Version</span>
                <p className="text-small mt-1 font-mono">{health?.version ?? 'Unknown'}</p>
              </div>
              <div>
                <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">Status</span>
                <div className="mt-1 flex items-center gap-2">
                  <StatusDot variant={health?.healthy ? 'success' : 'danger'} />
                  <span className={`text-small font-mono ${health?.healthy ? 'text-success' : 'text-danger'}`}>
                    {health?.healthy ? 'Running' : 'Offline'}
                  </span>
                </div>
              </div>
            </div>
          </CardContent>
        </Card>

        {/* Database Card */}
        <Card className="border-border-subtle bg-background/60">
          <CardHeader>
            <CardTitle className="flex items-center gap-2.5">
              <Database className="text-foreground-muted h-5 w-5" />
              Database
            </CardTitle>
            <CardDescription>SQLite state storage</CardDescription>
          </CardHeader>
          <CardContent>
            <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">Status</span>
            <div className="mt-1 flex items-center gap-2">
              <StatusDot variant={health?.database_ok ? 'success' : 'danger'} />
              <span className={`text-small font-mono ${health?.database_ok ? 'text-success' : 'text-danger'}`}>
                {health?.database_ok ? 'Healthy' : 'Error'}
              </span>
            </div>
          </CardContent>
        </Card>

        {/* Repository Policies Card */}
        <Card className="border-border-subtle bg-background/60">
          <CardHeader>
            <div className="flex items-center justify-between">
              <div>
                <CardTitle className="flex items-center gap-2.5">
                  <GitBranch className="text-foreground-muted h-5 w-5" />
                  Repository Workflows
                </CardTitle>
                <CardDescription>Per-repository workflow configuration</CardDescription>
              </div>
              <Button variant="ghost" size="sm" onClick={refreshRepos} disabled={reposLoading}>
                <RefreshCw className={`h-4 w-4 ${reposLoading ? 'animate-spin' : ''}`} />
              </Button>
            </div>
          </CardHeader>
          <CardContent>
            {reposLoading ? (
              <div className="text-foreground-muted flex items-center gap-2">
                <RefreshCw className="h-4 w-4 animate-spin" />
                Loading repositories...
              </div>
            ) : reposError ? (
              <div className="text-danger flex items-center gap-2">
                <AlertCircle className="h-4 w-4" />
                {reposError}
              </div>
            ) : repos.length === 0 ? (
              <div className="text-small text-foreground-muted">
                <p>No repositories enrolled.</p>
                <p className="mt-1">
                  Run <code className="bg-surface rounded px-1">airlock init</code> in a git repository to enroll it.
                </p>
              </div>
            ) : (
              <div className="space-y-2">
                <p className="text-micro text-foreground-muted mb-3">
                  Workflow files in <code className="bg-surface rounded px-1">.airlock/workflows/</code> define the
                  pipeline jobs and steps for each repository.
                </p>
                {repos.map((repo) => (
                  <RepoPolicyCard key={repo.id} repoId={repo.id} workingPath={repo.working_path} />
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
