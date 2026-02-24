import type { Meta, StoryObj } from '@storybook/react';
import { Atmosphere } from '../react/atmosphere';

const meta: Meta = {
  title: 'Primitives/Atmosphere/Scroll Test',
  parameters: {
    layout: 'fullscreen',
  },
};

export default meta;
type Story = StoryObj;

const paragraphs = [
  'Airlock is a local-first CI system that transforms messy AI-generated code into clean, reviewable pull requests. It intercepts git push operations through a local bare repo "gate", runs a transformation pipeline, and lets users review changes before forwarding to upstream.',
  'The system works by installing a local bare repository that acts as a gateway between your working repository and the upstream remote. When you push, Airlock intercepts the changes, creates an isolated worktree, and runs your configured pipeline stages against the code.',
  'Each pipeline run produces a detailed report of what was checked, what passed, and what failed. You can configure stages for linting, formatting, testing, security scanning, and any other validation step your project requires.',
  'The daemon process manages pipeline execution in the background, communicating with the desktop application over a Unix socket. This architecture keeps the UI responsive while heavy operations like test suites run asynchronously.',
  'Worktrees are created fresh for each run, ensuring complete isolation between pipeline executions. Artifacts from each stage are stored locally for later inspection, and results are recorded in a SQLite database for history and analytics.',
  'The desktop application provides a real-time view of active and past runs. You can drill into individual stages, view logs, inspect diffs, and approve or reject changes before they reach your upstream repository.',
  'Configuration is managed through a simple YAML file at the repository root. You define your stages, their order, and any parameters they need. Airlock ships with built-in step definitions for common tasks, but you can also define custom steps using shell commands.',
  'The enrollment process is straightforward: run airlock init in your repository, and it reconfigures your git remotes so pushes flow through the local gate. To remove Airlock, run airlock eject and everything reverts to its original state.',
  'Because everything runs locally, there are no cloud dependencies, no waiting for remote runners, and no sharing sensitive code with third-party services. Your code never leaves your machine until you explicitly approve the push.',
  'Airlock is designed for high-velocity agentic engineering workflows where AI tools generate large volumes of code changes. It provides a structured review checkpoint that catches issues before they pollute your git history.',
];

export const TallPage: Story = {
  render: () => (
    <div className="bg-background relative min-h-screen">
      <Atmosphere />

      <div className="relative z-10 mx-auto max-w-2xl px-8 py-16">
        <header className="mb-12">
          <p className="text-small text-foreground-muted mb-2 font-mono tracking-widest uppercase">Engineering Blog</p>
          <h1 className="text-display text-foreground mb-4" style={{ letterSpacing: '-0.02em' }}>
            Building a Local-First CI System
          </h1>
          <p className="text-body text-foreground-muted">
            How Airlock transforms AI-generated code into clean pull requests without ever leaving your machine.
          </p>
          <div className="border-border mt-6 border-t" />
        </header>

        <article className="space-y-8">
          {paragraphs.map((text, i) => (
            <div key={i}>
              <h2 className="text-heading text-foreground mb-3">Section {i + 1}</h2>
              <p className="text-body text-foreground-muted leading-relaxed">{text}</p>
            </div>
          ))}
        </article>

        <footer className="border-border mt-16 border-t pt-8 pb-16">
          <p className="text-small text-foreground-muted font-mono">
            End of article — the atmosphere should still be visible behind this content.
          </p>
        </footer>
      </div>
    </div>
  ),
};
