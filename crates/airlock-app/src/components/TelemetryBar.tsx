import { StatusDot, type StatusDotProps } from '@airlock-hq/design-system/react';
import { useDaemonHealth, useRepos, useAllRuns } from '@/hooks/use-daemon';
import { isTauri } from '@/lib/tauri';

/**
 * Telemetry status bar showing daemon health, execution mode, and enrolled repo count.
 * Uses telemetry-style typography: mono, micro, uppercase, tracking-widest.
 */
export function TelemetryBar() {
  const { health, loading: healthLoading } = useDaemonHealth();
  const { repos } = useRepos();
  const { runs } = useAllRuns(50);

  const daemonStatus = healthLoading ? 'loading' : health?.healthy ? 'healthy' : 'offline';
  const repoCount = repos.length;
  const executingCount = runs.filter((r) => r.status === 'running').length;
  const awaitingCount = runs.filter((r) => r.status === 'pending_review' || r.status === 'awaiting_approval').length;

  const items: { label: string; value: string; dot?: boolean; variant?: StatusDotProps['variant']; pulse?: boolean }[] =
    [
      {
        label: 'DAEMON',
        value: isTauri
          ? daemonStatus === 'healthy'
            ? 'ONLINE'
            : daemonStatus === 'loading'
              ? 'CONNECTING...'
              : 'OFFLINE'
          : 'MOCK',
        dot: true,
        variant: isTauri
          ? daemonStatus === 'healthy'
            ? 'success'
            : daemonStatus === 'loading'
              ? 'signal'
              : 'danger'
          : 'warning',
        pulse: isTauri && daemonStatus === 'loading',
      },
      { label: 'REPOS', value: `${repoCount} ENROLLED` },
      {
        label: 'EXECUTING',
        value: String(executingCount),
        dot: true,
        variant: executingCount > 0 ? 'warning' : 'muted',
        pulse: executingCount > 0,
      },
      {
        label: 'AWAITING',
        value: String(awaitingCount),
        dot: true,
        variant: awaitingCount > 0 ? 'signal' : 'muted',
        pulse: awaitingCount > 0,
      },
    ];

  return (
    <div className="border-border-subtle flex items-center gap-0 border-b px-8 py-3">
      {items.map((item, i) => (
        <div key={item.label} className="flex items-center">
          {i > 0 && <div className="bg-border-subtle mx-6 h-4 w-px" />}
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
