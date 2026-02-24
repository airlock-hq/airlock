import type { Meta, StoryObj } from '@storybook/react';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '../react/tabs';

const meta: Meta = {
  title: 'Primitives/Tabs',
  parameters: { layout: 'padded' },
};

export default meta;
type Story = StoryObj;

export const Default: Story = {
  render: () => (
    <Tabs defaultValue="overview" className="w-[400px]">
      <TabsList>
        <TabsTrigger value="overview">Overview</TabsTrigger>
        <TabsTrigger value="changes">Changes</TabsTrigger>
        <TabsTrigger value="logs">Logs</TabsTrigger>
      </TabsList>
      <TabsContent value="overview">
        <p className="text-small text-foreground-muted">Overview content goes here.</p>
      </TabsContent>
      <TabsContent value="changes">
        <p className="text-small text-foreground-muted">Changes content goes here.</p>
      </TabsContent>
      <TabsContent value="logs">
        <p className="text-small text-foreground-muted">Logs content goes here.</p>
      </TabsContent>
    </Tabs>
  ),
};

export const Line: Story = {
  render: () => (
    <Tabs defaultValue="overview" className="w-[400px]">
      <TabsList variant="line">
        <TabsTrigger variant="line" value="overview">
          Overview
        </TabsTrigger>
        <TabsTrigger variant="line" value="changes">
          Changes
        </TabsTrigger>
        <TabsTrigger variant="line" value="logs">
          Logs
        </TabsTrigger>
      </TabsList>
      <TabsContent value="overview">
        <p className="text-small text-foreground-muted mt-4">Overview content goes here.</p>
      </TabsContent>
      <TabsContent value="changes">
        <p className="text-small text-foreground-muted mt-4">Changes content goes here.</p>
      </TabsContent>
      <TabsContent value="logs">
        <p className="text-small text-foreground-muted mt-4">Logs content goes here.</p>
      </TabsContent>
    </Tabs>
  ),
};
