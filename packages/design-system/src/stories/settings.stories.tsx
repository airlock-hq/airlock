import type { Meta, StoryObj } from '@storybook/react';
import { useState } from 'react';
import { Button } from '../react/button';
import { Badge } from '../react/badge';
import { Input } from '../react/input';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../react/card';
import { StatusDot } from '../react/status-dot';
import { Atmosphere } from '../react/atmosphere';
import {
  Server,
  Database,
  Folder,
  Pencil,
  X,
  Save,
  RefreshCw,
  GitBranch,
  Plus,
  Trash2,
  ChevronDown,
  ChevronRight,
} from 'lucide-react';

const meta: Meta = {
  title: 'Pages/Settings',
  parameters: {
    layout: 'fullscreen',
  },
};

export default meta;
type Story = StoryObj;

/* ---------- Mock Data ---------- */

interface MockBranchMapping {
  pattern: string;
  pipeline: string;
}

interface MockRepoPolicy {
  id: string;
  shortPath: string;
  workingPath: string;
  configExists: boolean;
  branchMappings: MockBranchMapping[];
  pipelines: string[];
  alwaysSeparate: string[];
  keepTogether: string[][];
}

const mockRepoPolicies: MockRepoPolicy[] = [
  {
    id: 'repo-1',
    shortPath: 'acme/backend',
    workingPath: '/Users/dev/projects/acme/backend',
    configExists: true,
    branchMappings: [
      { pattern: 'main', pipeline: 'default' },
      { pattern: 'feature/*', pipeline: 'feature' },
      { pattern: 'hotfix/*', pipeline: 'hotfix' },
    ],
    pipelines: ['default', 'feature', 'hotfix'],
    alwaysSeparate: ['*.sql', 'migrations/*'],
    keepTogether: [['src/api/*.ts', 'src/api/*.test.ts'], ['docs/*.md']],
  },
  {
    id: 'repo-2',
    shortPath: 'acme/frontend',
    workingPath: '/Users/dev/projects/acme/frontend',
    configExists: false,
    branchMappings: [{ pattern: '*', pipeline: 'default' }],
    pipelines: ['default'],
    alwaysSeparate: [],
    keepTogether: [],
  },
];

/* ---------- Components ---------- */

function RepoPolicyCard({ policy, startEditing = false }: { policy: MockRepoPolicy; startEditing?: boolean }) {
  const [expanded, setExpanded] = useState(startEditing);
  const [editing, setEditing] = useState(startEditing);
  const [mappings, setMappings] = useState(policy.branchMappings);
  const [alwaysSeparate, setAlwaysSeparate] = useState(policy.alwaysSeparate);
  const [keepTogether, setKeepTogether] = useState(policy.keepTogether);

  return (
    <div className="border-border-subtle bg-background/40 rounded-lg border">
      <button
        type="button"
        className="hover:bg-surface/30 flex w-full items-center justify-between p-4 transition-colors"
        onClick={() => setExpanded(!expanded)}
      >
        <div className="flex items-center gap-2.5 text-left">
          {expanded ? (
            <ChevronDown className="text-foreground-muted h-4 w-4 shrink-0" />
          ) : (
            <ChevronRight className="text-foreground-muted h-4 w-4 shrink-0" />
          )}
          <span className="truncate font-medium">{policy.shortPath}</span>
          {policy.configExists && (
            <Badge variant="secondary" className="text-micro">
              Custom
            </Badge>
          )}
        </div>
      </button>

      {expanded && (
        <div className="border-border-subtle space-y-5 border-t p-4">
          <p className="text-micro text-foreground-muted truncate font-mono">{policy.workingPath}</p>

          {/* Branch Mappings */}
          <div>
            <div className="mb-3 flex items-center justify-between">
              <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">
                Branch Mappings
              </span>
              {editing && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-micro h-7"
                  onClick={() => setMappings([...mappings, { pattern: '', pipeline: 'default' }])}
                >
                  <Plus className="mr-1 h-3 w-3" /> Add
                </Button>
              )}
            </div>
            {editing ? (
              <div className="space-y-2">
                {mappings.map((m, i) => (
                  <div key={i} className="flex items-center gap-2">
                    <Input
                      className="border-border-subtle bg-background/60 text-small flex-1 font-mono"
                      placeholder="Branch pattern"
                      value={m.pattern}
                      onChange={(e: { target: { value: string } }) => {
                        const u = [...mappings];
                        u[i] = { ...u[i], pattern: e.target.value };
                        setMappings(u);
                      }}
                    />
                    <span className="text-foreground-muted">&rarr;</span>
                    <Input
                      className="border-border-subtle bg-background/60 text-small flex-1 font-mono"
                      placeholder="Pipeline"
                      value={m.pipeline}
                      onChange={(e: { target: { value: string } }) => {
                        const u = [...mappings];
                        u[i] = { ...u[i], pipeline: e.target.value };
                        setMappings(u);
                      }}
                    />
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => setMappings(mappings.filter((_, j) => j !== i))}
                      className="text-danger hover:text-danger"
                    >
                      <Trash2 className="h-3 w-3" />
                    </Button>
                  </div>
                ))}
              </div>
            ) : (
              <div className="space-y-1.5">
                {policy.branchMappings.map((m, i) => (
                  <div key={i} className="text-micro flex items-center gap-2 font-mono">
                    <code className="bg-surface/60 rounded px-1.5 py-0.5">{m.pattern}</code>
                    <span className="text-foreground-muted">&rarr;</span>
                    <code className="bg-surface/60 rounded px-1.5 py-0.5">{m.pipeline}</code>
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* Available Pipelines */}
          {policy.pipelines.length > 0 && (
            <div>
              <span className="text-micro text-foreground-muted mb-2 block font-mono tracking-widest uppercase">
                Pipelines
              </span>
              <div className="flex flex-wrap gap-1.5">
                {policy.pipelines.map((p) => (
                  <Badge key={p} variant="outline" className="text-micro font-mono">
                    {p}
                  </Badge>
                ))}
              </div>
            </div>
          )}

          {/* Always Separate */}
          <div>
            <div className="mb-2 flex items-center justify-between">
              <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">
                Always Separate
              </span>
              {editing && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-micro h-7"
                  onClick={() => setAlwaysSeparate([...alwaysSeparate, ''])}
                >
                  <Plus className="mr-1 h-3 w-3" /> Add
                </Button>
              )}
            </div>
            <p className="text-micro text-foreground-muted mb-2">
              Files matching these patterns will always be in separate intents.
            </p>
            {editing ? (
              <div className="space-y-2">
                {alwaysSeparate.length === 0 ? (
                  <p className="text-micro text-foreground-muted/60">No patterns configured.</p>
                ) : (
                  alwaysSeparate.map((pat, i) => (
                    <div key={i} className="flex items-center gap-2">
                      <Input
                        className="border-border-subtle bg-background/60 text-small flex-1 font-mono"
                        placeholder="e.g., *.sql"
                        value={pat}
                        onChange={(e: { target: { value: string } }) => {
                          const u = [...alwaysSeparate];
                          u[i] = e.target.value;
                          setAlwaysSeparate(u);
                        }}
                      />
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => setAlwaysSeparate(alwaysSeparate.filter((_, j) => j !== i))}
                        className="text-danger hover:text-danger"
                      >
                        <Trash2 className="h-3 w-3" />
                      </Button>
                    </div>
                  ))
                )}
              </div>
            ) : (
              <div className="flex flex-wrap gap-1.5">
                {policy.alwaysSeparate.length === 0 ? (
                  <p className="text-micro text-foreground-muted/60">No patterns configured.</p>
                ) : (
                  policy.alwaysSeparate.map((p, i) => (
                    <code key={i} className="bg-surface/60 text-micro rounded px-1.5 py-0.5 font-mono">
                      {p}
                    </code>
                  ))
                )}
              </div>
            )}
          </div>

          {/* Keep Together */}
          <div>
            <div className="mb-2 flex items-center justify-between">
              <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">
                Keep Together
              </span>
              {editing && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-micro h-7"
                  onClick={() => setKeepTogether([...keepTogether, ['']])}
                >
                  <Plus className="mr-1 h-3 w-3" /> Add Group
                </Button>
              )}
            </div>
            <p className="text-micro text-foreground-muted mb-2">
              Files in the same group will be kept in the same intent.
            </p>
            {editing ? (
              <div className="space-y-3">
                {keepTogether.length === 0 ? (
                  <p className="text-micro text-foreground-muted/60">No groups configured.</p>
                ) : (
                  keepTogether.map((group, gi) => (
                    <div key={gi} className="border-border-subtle bg-background/30 space-y-2 rounded-lg border p-3">
                      <div className="flex items-center justify-between">
                        <span className="text-micro text-foreground-muted font-mono">GROUP {gi + 1}</span>
                        <div className="flex gap-1">
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-6 w-6 p-0"
                            onClick={() => {
                              const u = [...keepTogether];
                              u[gi] = [...u[gi], ''];
                              setKeepTogether(u);
                            }}
                          >
                            <Plus className="h-3 w-3" />
                          </Button>
                          <Button
                            variant="ghost"
                            size="sm"
                            className="text-danger hover:text-danger h-6 w-6 p-0"
                            onClick={() => setKeepTogether(keepTogether.filter((_, j) => j !== gi))}
                          >
                            <Trash2 className="h-3 w-3" />
                          </Button>
                        </div>
                      </div>
                      {group.map((pat, pi) => (
                        <div key={pi} className="flex items-center gap-2">
                          <Input
                            className="border-border-subtle bg-background/60 text-small flex-1 font-mono"
                            placeholder="e.g., src/api/*.ts"
                            value={pat}
                            onChange={(e: { target: { value: string } }) => {
                              const u = [...keepTogether];
                              u[gi] = [...u[gi]];
                              u[gi][pi] = e.target.value;
                              setKeepTogether(u);
                            }}
                          />
                          <Button
                            variant="ghost"
                            size="sm"
                            className="text-danger hover:text-danger"
                            onClick={() => {
                              const u = [...keepTogether];
                              u[gi] = u[gi].filter((_, j) => j !== pi);
                              setKeepTogether(u);
                            }}
                          >
                            <Trash2 className="h-3 w-3" />
                          </Button>
                        </div>
                      ))}
                    </div>
                  ))
                )}
              </div>
            ) : (
              <div className="space-y-2">
                {policy.keepTogether.length === 0 ? (
                  <p className="text-micro text-foreground-muted/60">No groups configured.</p>
                ) : (
                  policy.keepTogether.map((group, gi) => (
                    <div key={gi} className="flex flex-wrap items-center gap-1.5">
                      <span className="text-micro text-foreground-muted mr-1 font-mono">GROUP {gi + 1}:</span>
                      {group.map((p, pi) => (
                        <code key={pi} className="bg-surface/60 text-micro rounded px-1.5 py-0.5 font-mono">
                          {p}
                        </code>
                      ))}
                    </div>
                  ))
                )}
              </div>
            )}
          </div>

          {/* Actions */}
          <div className="border-border-subtle flex justify-end gap-2 border-t pt-3">
            {editing ? (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  className="border-border-subtle text-micro h-8"
                  onClick={() => setEditing(false)}
                >
                  <X className="mr-1 h-3.5 w-3.5" /> Cancel
                </Button>
                <Button size="sm" className="text-micro h-8" onClick={() => setEditing(false)}>
                  <Save className="mr-1 h-3.5 w-3.5" /> Save
                </Button>
              </>
            ) : (
              <Button variant="ghost" size="sm" className="text-micro h-8" onClick={() => setEditing(true)}>
                <Pencil className="mr-1 h-3.5 w-3.5" /> Edit
              </Button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function SettingsPage({ editingRepo }: { editingRepo?: string }) {
  return (
    <div className="bg-background relative min-h-screen">
      <Atmosphere />

      <div className="relative z-10 px-8 py-8">
        {/* Header */}
        <div className="mb-8">
          <h1 className="text-h1 text-foreground font-bold tracking-tight">Settings</h1>
          <p className="text-small text-foreground-muted mt-1">Configure Airlock preferences</p>
        </div>

        <div className="grid gap-5">
          {/* Daemon Information */}
          <Card className="border-border-subtle bg-background/60">
            <CardHeader>
              <CardTitle className="flex items-center gap-2.5">
                <Server className="text-foreground-muted h-5 w-5" /> Daemon Information
              </CardTitle>
              <CardDescription>Current Airlock daemon status</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-2 gap-8">
                <div>
                  <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">Version</span>
                  <p className="text-small mt-1 font-mono">0.4.1</p>
                </div>
                <div>
                  <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">Status</span>
                  <div className="mt-1 flex items-center gap-2">
                    <StatusDot variant="success" />
                    <span className="text-small text-success font-mono">Running</span>
                  </div>
                </div>
              </div>
            </CardContent>
          </Card>

          {/* Database */}
          <Card className="border-border-subtle bg-background/60">
            <CardHeader>
              <CardTitle className="flex items-center gap-2.5">
                <Database className="text-foreground-muted h-5 w-5" /> Database
              </CardTitle>
              <CardDescription>SQLite state storage</CardDescription>
            </CardHeader>
            <CardContent>
              <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">Status</span>
              <div className="mt-1 flex items-center gap-2">
                <div className="bg-success h-1.5 w-1.5 rounded-full" />
                <span className="text-small text-success font-mono">Healthy</span>
              </div>
            </CardContent>
          </Card>

          {/* File Locations */}
          <Card className="border-border-subtle bg-background/60">
            <CardHeader>
              <CardTitle className="flex items-center gap-2.5">
                <Folder className="text-foreground-muted h-5 w-5" /> File Locations
              </CardTitle>
              <CardDescription>Where Airlock stores its data</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-2 gap-x-12 gap-y-4">
                {[
                  { label: 'CONFIGURATION', path: '~/.airlock/config.toml' },
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

          {/* Repository Policies */}
          <Card className="border-border-subtle bg-background/60">
            <CardHeader>
              <div className="flex items-center justify-between">
                <div>
                  <CardTitle className="flex items-center gap-2.5">
                    <GitBranch className="text-foreground-muted h-5 w-5" /> Repository Policies
                  </CardTitle>
                  <CardDescription>Per-repository workflow configuration</CardDescription>
                </div>
                <Button variant="ghost" size="sm">
                  <RefreshCw className="h-4 w-4" />
                </Button>
              </div>
            </CardHeader>
            <CardContent>
              <p className="text-micro text-foreground-muted mb-4">
                Workflow files in <code className="bg-surface/60 rounded px-1">.airlock/workflows/</code> define the
                pipeline jobs and steps for each repository.
              </p>
              <div className="space-y-3">
                {mockRepoPolicies.map((policy) => (
                  <RepoPolicyCard key={policy.id} policy={policy} startEditing={editingRepo === policy.id} />
                ))}
              </div>
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  );
}

/* ---------- Stories ---------- */

export const Default: Story = {
  render: () => <SettingsPage />,
};

export const Editing: Story = {
  render: () => <SettingsPage editingRepo="repo-1" />,
};
