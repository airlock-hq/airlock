import type { Meta, StoryObj } from '@storybook/react';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../react/select';
import { Label } from '../react/label';

const meta: Meta = {
  title: 'Primitives/Select',
  parameters: { layout: 'centered' },
};

export default meta;
type Story = StoryObj;

export const Default: Story = {
  render: () => (
    <Select>
      <SelectTrigger className="w-[200px]">
        <SelectValue placeholder="Select stage..." />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="lint">Lint</SelectItem>
        <SelectItem value="test">Test</SelectItem>
        <SelectItem value="describe">Describe</SelectItem>
        <SelectItem value="create-pr">Create PR</SelectItem>
        <SelectItem value="push">Push</SelectItem>
      </SelectContent>
    </Select>
  ),
};

export const WithLabel: Story = {
  render: () => (
    <div className="grid gap-1.5">
      <Label>Pipeline stage</Label>
      <Select defaultValue="lint">
        <SelectTrigger className="w-[200px]">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="lint">Lint</SelectItem>
          <SelectItem value="test">Test</SelectItem>
          <SelectItem value="describe">Describe</SelectItem>
        </SelectContent>
      </Select>
    </div>
  ),
};
