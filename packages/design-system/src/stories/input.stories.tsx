import type { Meta, StoryObj } from '@storybook/react';
import { Input } from '../react/input';
import { Label } from '../react/label';
import { Textarea } from '../react/textarea';

const meta: Meta = {
  title: 'Primitives/Input',
  parameters: { layout: 'centered' },
};

export default meta;
type Story = StoryObj;

export const Default: Story = {
  render: () => <Input placeholder="Enter text..." className="w-[300px]" />,
};

export const Disabled: Story = {
  render: () => <Input placeholder="Disabled" disabled className="w-[300px]" />,
};

export const WithLabel: Story = {
  render: () => (
    <div className="grid w-[300px] gap-1.5">
      <Label htmlFor="repo">Repository URL</Label>
      <Input id="repo" placeholder="https://github.com/..." />
    </div>
  ),
};

export const TextArea: Story = {
  name: 'Textarea',
  render: () => (
    <div className="grid w-[300px] gap-1.5">
      <Label htmlFor="description">Description</Label>
      <Textarea id="description" placeholder="Describe your changes..." />
    </div>
  ),
};

export const TextAreaDisabled: Story = {
  name: 'Textarea Disabled',
  render: () => <Textarea placeholder="Disabled" disabled className="w-[300px]" />,
};
