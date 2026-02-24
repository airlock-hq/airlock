/** Shared status-to-visual-config mapping for consistent pipeline status display. */

export interface StatusConfig {
  variant: 'success' | 'danger' | 'warning' | 'signal' | 'muted';
  pulse?: boolean;
  barColor: string;
}

/** Get visual config (dot variant, pulse, bar color) for a pipeline status string. */
export function getStatusConfig(status: string): StatusConfig {
  switch (status) {
    case 'passed':
      return { variant: 'success', barColor: 'bg-success/70' };
    case 'failed':
      return { variant: 'danger', barColor: 'bg-danger/70' };
    case 'running':
      return { variant: 'warning', pulse: true, barColor: 'bg-warning/70 animate-pulse' };
    case 'awaiting_approval':
      return { variant: 'signal', pulse: true, barColor: 'bg-signal/70 animate-pulse' };
    case 'skipped':
      return { variant: 'muted', barColor: 'bg-foreground-muted/20' };
    case 'pending':
    default:
      return { variant: 'muted', barColor: 'bg-foreground-muted/15' };
  }
}
