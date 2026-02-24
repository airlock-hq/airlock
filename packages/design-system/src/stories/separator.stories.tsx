import type { Meta, StoryObj } from '@storybook/react';
import { Separator } from '../react/separator';

const meta: Meta<typeof Separator> = {
  title: 'Primitives/Separator',
  component: Separator,
};

export default meta;
type Story = StoryObj<typeof meta>;

export const Horizontal: Story = {
  render: () => (
    <div className="w-[300px]">
      <p className="text-small">Above</p>
      <Separator className="my-4" />
      <p className="text-small">Below</p>
    </div>
  ),
};

export const Vertical: Story = {
  render: () => (
    <div className="flex h-8 items-center gap-4">
      <span className="text-small">Left</span>
      <Separator orientation="vertical" />
      <span className="text-small">Right</span>
    </div>
  ),
};
