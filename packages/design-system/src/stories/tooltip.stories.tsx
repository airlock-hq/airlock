import type { Meta, StoryObj } from '@storybook/react';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../react/tooltip';
import { Button } from '../react/button';

const meta: Meta = {
  title: 'Primitives/Tooltip',
  parameters: { layout: 'centered' },
};

export default meta;
type Story = StoryObj;

export const Default: Story = {
  render: () => (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button variant="outline">Hover me</Button>
        </TooltipTrigger>
        <TooltipContent>
          <p>Pipeline status: running</p>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  ),
};
