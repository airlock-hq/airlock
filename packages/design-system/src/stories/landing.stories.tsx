import type { Meta, StoryObj } from '@storybook/react';
import { Button } from '../react/button';
import { StatusDot } from '../react/status-dot';
import { Atmosphere } from '../react/atmosphere';

const meta: Meta = {
  title: 'Pages/Landing',
  parameters: {
    layout: 'fullscreen',
  },
};

export default meta;
type Story = StoryObj;

/* ---------- Telemetry Bar ---------- */

function TelemetryBar() {
  const items = [
    { label: 'SYSTEM STATUS', value: 'VALIDATING...', dot: true },
    { label: 'MODE', value: 'LOCAL_EXECUTION' },
    { label: 'SYNC', value: 'ACTIVE' },
  ];

  return (
    <div className="border-border flex items-center justify-center gap-0 border-t px-8 py-4">
      {items.map((item, i) => (
        <div key={item.label} className="flex items-center">
          {i > 0 && <div className="bg-border mx-6 h-4 w-px" />}
          <div className="flex items-center gap-2">
            {item.dot && <StatusDot variant="signal" size="md" pulse />}
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

/* ---------- Landing Page ---------- */

export const Landing: Story = {
  render: () => (
    <div className="bg-background relative flex min-h-screen flex-col">
      {/* Atmospheric layers */}
      <Atmosphere />

      {/* Content */}
      <div className="relative z-10 flex flex-1 flex-col">
        {/* Header */}
        <header className="flex items-center justify-center px-8 pt-10">
          <span className="text-small text-foreground-muted font-mono tracking-[0.3em] uppercase">Airlock</span>
        </header>

        {/* Hero */}
        <main className="flex flex-1 flex-col items-center justify-center gap-6 px-8">
          <h1 className="text-display text-foreground text-center" style={{ letterSpacing: '-0.02em' }}>
            Autonomous CI/CD. Running in your orbit.
          </h1>
          <p className="text-body text-foreground-muted text-center">
            Initialize. Validate. Deploy. Without leaving your machine.
          </p>
          <div className="mt-4">
            <Button variant="signal" size="lg">
              Initialize System
            </Button>
          </div>
        </main>

        {/* Telemetry footer */}
        <footer>
          <TelemetryBar />
        </footer>
      </div>

      {/* Inline keyframes for the pulse animation */}
      <style>{`
        @keyframes pulse {
          0%, 100% { opacity: 1; }
          50% { opacity: 0.4; }
        }
      `}</style>
    </div>
  ),
};
