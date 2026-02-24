import type { Meta, StoryObj } from '@storybook/react';
import { Button } from '../react/button';

const meta: Meta<typeof Button> = {
  title: 'Primitives/Button',
  component: Button,
  parameters: { layout: 'centered' },
};

export default meta;
type Story = StoryObj<typeof meta>;

export const Neutral: Story = {
  args: { children: 'Neutral', variant: 'default' },
};
export const Signal: Story = {
  args: { children: 'Signal', variant: 'signal' },
};
export const Danger: Story = {
  args: { children: 'Danger', variant: 'danger' },
};
export const Outline: Story = {
  args: { children: 'Outline', variant: 'outline' },
};
export const Secondary: Story = {
  args: { children: 'Secondary', variant: 'secondary' },
};
export const Ghost: Story = {
  args: { children: 'Ghost', variant: 'ghost' },
};
export const Link: Story = {
  args: { children: 'Link', variant: 'link' },
};

export const AllVariants: Story = {
  render: () => (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 24 }}>
      <div>
        <p className="text-small text-foreground-muted mb-3">Variants</p>
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <Button variant="default">Neutral</Button>
          <Button variant="signal">Signal</Button>
          <Button variant="danger">Danger</Button>
          <Button variant="outline">Outline</Button>
          <Button variant="secondary">Secondary</Button>
          <Button variant="ghost">Ghost</Button>
          <Button variant="link">Link</Button>
        </div>
      </div>
      <div>
        <p className="text-small text-foreground-muted mb-3">Sizes</p>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <Button size="sm">Small</Button>
          <Button size="default">Default</Button>
          <Button size="lg">Large</Button>
          <Button size="icon">A</Button>
        </div>
      </div>
      <div>
        <p className="text-small text-foreground-muted mb-3">Disabled</p>
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          <Button variant="default" disabled>
            Neutral
          </Button>
          <Button variant="signal" disabled>
            Signal
          </Button>
          <Button variant="danger" disabled>
            Danger
          </Button>
          <Button variant="outline" disabled>
            Outline
          </Button>
        </div>
      </div>
    </div>
  ),
};
