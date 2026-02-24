import type { Meta, StoryObj } from '@storybook/react';
import { StatusDot } from '../react/status-dot';
import { Atmosphere } from '../react/atmosphere';
import { TooltipProvider } from '../react/tooltip';

const meta: Meta = {
  title: 'Pages/App Shell',
  parameters: {
    layout: 'fullscreen',
  },
};

export default meta;
type Story = StoryObj;

/* ---------- Telemetry Bar ---------- */

function TelemetryBar({
  daemonStatus = 'healthy',
  mockMode = false,
  repoCount = 3,
}: {
  daemonStatus?: 'healthy' | 'offline' | 'loading';
  mockMode?: boolean;
  repoCount?: number;
}) {
  const items: {
    label: string;
    value: string;
    dot?: boolean;
    variant?: 'success' | 'danger' | 'warning' | 'signal' | 'muted';
    pulse?: boolean;
  }[] = [
    {
      label: 'DAEMON',
      value: mockMode
        ? 'MOCK'
        : daemonStatus === 'healthy'
          ? 'ONLINE'
          : daemonStatus === 'loading'
            ? 'CONNECTING...'
            : 'OFFLINE',
      dot: true,
      variant: mockMode
        ? 'warning'
        : daemonStatus === 'healthy'
          ? 'success'
          : daemonStatus === 'loading'
            ? 'signal'
            : 'danger',
      pulse: !mockMode && daemonStatus === 'loading',
    },
    { label: 'REPOS', value: `${repoCount} ENROLLED` },
  ];

  return (
    <div className="border-border-subtle flex items-center gap-0 border-b px-8 py-3">
      {items.map((item, i) => (
        <div key={item.label} className="flex items-center">
          {i > 0 && <div className="bg-border mx-6 h-4 w-px" />}
          <div className="flex items-center gap-2">
            {item.dot && <StatusDot variant={item.variant} pulse={item.pulse} />}
            <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">{item.label}:</span>
            <span className="text-micro text-foreground font-mono font-semibold tracking-widest uppercase">
              {item.value}
            </span>
          </div>
        </div>
      ))}
    </div>
  );
}

/* ---------- Nav Tab ---------- */

function NavTab({ active, children }: { active?: boolean; children: React.ReactNode }) {
  return (
    <button
      className={`text-small flex items-center px-4 py-2 transition-colors ${
        active ? 'text-foreground font-medium' : 'text-foreground-muted hover:text-foreground'
      }`}
    >
      {children}
    </button>
  );
}

/* ---------- Shell ---------- */

function AppShell({
  daemonStatus = 'healthy',
  mockMode = false,
  repoCount = 3,
  children,
}: {
  daemonStatus?: 'healthy' | 'offline' | 'loading';
  mockMode?: boolean;
  repoCount?: number;
  children?: React.ReactNode;
}) {
  return (
    <TooltipProvider>
      <div className="bg-background relative flex h-screen flex-col">
        {/* Atmospheric layers */}
        <Atmosphere />

        {/* Content */}
        <div className="relative z-10 flex flex-1 flex-col">
          {/* Top Navigation Bar */}
          <div className="flex h-16 items-center justify-between px-8">
            {/* Left: Logo */}
            <div className="flex items-center gap-3">
              <span className="text-h2 text-foreground-muted font-sans font-medium tracking-[0.2em] uppercase">
                Airlock
              </span>
            </div>

            {/* Right: Navigation */}
            <nav className="flex items-center gap-2">
              <NavTab active>Runs</NavTab>
              <span className="text-border">&middot;</span>
              <NavTab>Repositories</NavTab>
              <span className="text-border">&middot;</span>
              <NavTab>Settings</NavTab>
            </nav>
          </div>

          {/* Telemetry Status Bar */}
          <TelemetryBar daemonStatus={daemonStatus} mockMode={mockMode} repoCount={repoCount} />

          {/* Main Content */}
          <div className="flex flex-1 overflow-hidden">
            <main className="flex-1 overflow-auto px-8 py-6">
              {children || (
                <div className="flex h-full items-center justify-center">
                  <span className="text-micro text-foreground-muted font-mono tracking-widest uppercase">
                    SELECT A VIEW
                  </span>
                </div>
              )}
            </main>
          </div>
        </div>

        <style>{`
          @keyframes pulse {
            0%, 100% { opacity: 1; }
            50% { opacity: 0.4; }
          }
        `}</style>
      </div>
    </TooltipProvider>
  );
}

/* ---------- Stories ---------- */

export const Default: Story = {
  render: () => <AppShell />,
};

export const DaemonOffline: Story = {
  render: () => (
    <AppShell daemonStatus="offline" repoCount={0}>
      <div className="flex h-full flex-col items-center justify-center gap-6">
        <StatusDot variant="danger" size="md" />
        <span className="text-micro text-danger font-mono tracking-widest uppercase">DAEMON OFFLINE</span>
        <p className="text-small text-foreground-muted">
          Start the daemon with <code className="bg-surface text-micro rounded px-1.5 py-0.5 font-mono">airlockd</code>
        </p>
      </div>
    </AppShell>
  ),
};

export const MockMode: Story = {
  render: () => (
    <AppShell mockMode repoCount={2}>
      <div className="flex h-full flex-col items-center justify-center gap-6">
        <span className="text-micro text-warning font-mono tracking-widest uppercase">MOCK MODE ACTIVE</span>
        <p className="text-small text-foreground-muted max-w-sm text-center">
          Running with simulated data. Tauri backend is not available.
        </p>
      </div>
    </AppShell>
  ),
};
