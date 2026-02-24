import type { Meta, StoryObj } from '@storybook/react';
import { Badge } from '../react/badge';

const meta: Meta<typeof Badge> = {
  title: 'Primitives/Badge',
  component: Badge,
  parameters: { layout: 'centered' },
};

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: { children: 'Default' },
};
export const Signal: Story = {
  args: { children: 'Running', variant: 'signal' },
};
export const Success: Story = {
  args: { children: 'Passed', variant: 'success' },
};
export const Warning: Story = {
  args: { children: 'Pending', variant: 'warning' },
};
export const Danger: Story = {
  args: { children: 'Failed', variant: 'danger' },
};
export const Secondary: Story = {
  args: { children: 'Secondary', variant: 'secondary' },
};
export const Outline: Story = {
  args: { children: 'Outline', variant: 'outline' },
};

export const AllVariants: Story = {
  render: () => (
    <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
      <Badge variant="default">Default</Badge>
      <Badge variant="signal">Running</Badge>
      <Badge variant="success">Passed</Badge>
      <Badge variant="warning">Pending</Badge>
      <Badge variant="danger">Failed</Badge>
      <Badge variant="secondary">Secondary</Badge>
      <Badge variant="outline">Outline</Badge>
    </div>
  ),
};
