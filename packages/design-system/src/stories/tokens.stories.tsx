import type { Meta, StoryObj } from '@storybook/react';

const meta: Meta = {
  title: 'Foundation/Tokens',
};

export default meta;
type Story = StoryObj;

const colorSwatches = [
  { group: 'Surfaces', tokens: ['background', 'surface', 'surface-elevated'] },
  { group: 'Text', tokens: ['foreground', 'foreground-muted'] },
  { group: 'Borders', tokens: ['border', 'border-subtle'] },
  {
    group: 'Signal',
    tokens: ['signal', 'signal-subtle', 'signal-glow', 'signal-active', 'signal-focus', 'signal-attention'],
  },
  { group: 'Atmosphere', tokens: ['atmosphere-orbit-line'] },
  { group: 'Status', tokens: ['success', 'warning', 'danger'] },
  { group: 'Focus', tokens: ['ring'] },
  { group: 'Terminal', tokens: ['terminal', 'terminal-foreground', 'terminal-muted', 'terminal-border'] },
];

export const Palette: Story = {
  render: () => (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 32 }}>
      {colorSwatches.map(({ group, tokens }) => (
        <div key={group}>
          <h3 style={{ fontFamily: 'var(--font-family-sans)', fontSize: 14, fontWeight: 600, marginBottom: 12 }}>
            {group}
          </h3>
          <div style={{ display: 'grid', gap: 12, gridTemplateColumns: 'repeat(auto-fill, minmax(180px, 1fr))' }}>
            {tokens.map((name) => (
              <div
                key={name}
                style={{
                  border: '1px solid hsl(var(--border))',
                  borderRadius: 8,
                  padding: 12,
                  background: 'hsl(var(--surface))',
                }}
              >
                <div
                  style={{
                    height: 56,
                    borderRadius: 6,
                    background: `hsl(var(--${name}))`,
                    border: '1px solid hsl(var(--border-subtle))',
                  }}
                />
                <div style={{ marginTop: 8, fontFamily: 'var(--font-family-mono)', fontSize: 12 }}>{`--${name}`}</div>
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  ),
};

const typeScale = [
  { name: 'Display', class: 'text-display', sample: 'Airlock Control' },
  { name: 'H1', class: 'text-h1', sample: 'Page Heading' },
  { name: 'H2', class: 'text-h2', sample: 'Section Heading' },
  { name: 'Body', class: 'text-body', sample: 'Default body text for content and descriptions.' },
  { name: 'Small', class: 'text-small', sample: 'UI labels and secondary text' },
  { name: 'Micro', class: 'text-micro font-mono uppercase', sample: 'SYSTEM STATUS — ACTIVE' },
];

export const Typography: Story = {
  render: () => (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 24 }}>
      <div>
        <h3 style={{ fontFamily: 'var(--font-family-sans)', fontSize: 14, fontWeight: 600, marginBottom: 16 }}>
          Type Scale
        </h3>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
          {typeScale.map(({ name, class: cls, sample }) => (
            <div key={name} style={{ display: 'flex', alignItems: 'baseline', gap: 16 }}>
              <span
                style={{
                  fontFamily: 'var(--font-family-mono)',
                  fontSize: 11,
                  color: 'hsl(var(--foreground-muted))',
                  width: 80,
                  flexShrink: 0,
                }}
              >
                {name}
              </span>
              <span className={cls}>{sample}</span>
            </div>
          ))}
        </div>
      </div>
      <div>
        <h3 style={{ fontFamily: 'var(--font-family-sans)', fontSize: 14, fontWeight: 600, marginBottom: 16 }}>
          Font Families
        </h3>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <div>
            <span
              style={{ fontFamily: 'var(--font-family-mono)', fontSize: 11, color: 'hsl(var(--foreground-muted))' }}
            >
              --font-sans
            </span>
            <p className="text-body font-sans">The quick brown fox jumps over the lazy dog. 0123456789</p>
          </div>
          <div>
            <span
              style={{ fontFamily: 'var(--font-family-mono)', fontSize: 11, color: 'hsl(var(--foreground-muted))' }}
            >
              --font-mono
            </span>
            <p className="text-body font-mono">The quick brown fox jumps over the lazy dog. 0123456789</p>
          </div>
        </div>
      </div>
    </div>
  ),
};
