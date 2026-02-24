import type { Meta, StoryObj } from '@storybook/react';
import { OrbitalLines, RadialGlow, ParticleField, Atmosphere } from '../react/atmosphere';

const meta: Meta = {
  title: 'Primitives/Atmosphere',
  parameters: {
    layout: 'fullscreen',
  },
};

export default meta;
type Story = StoryObj;

export const Orbital: Story = {
  render: () => (
    <div className="bg-background relative h-[800px] w-full">
      <OrbitalLines />
    </div>
  ),
};

export const Glow: Story = {
  render: () => (
    <div className="bg-background relative h-[800px] w-full">
      <RadialGlow />
    </div>
  ),
};

export const Particles: Story = {
  render: () => (
    <div className="bg-background relative h-[800px] w-full">
      <ParticleField />
    </div>
  ),
};

export const All: Story = {
  render: () => <Atmosphere />,
};
