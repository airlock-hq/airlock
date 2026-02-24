import type { Meta, StoryObj } from '@storybook/react';
import { Card, CardHeader, CardTitle, CardDescription, CardContent, CardFooter } from '../react/card';
import { Button } from '../react/button';
import { Badge } from '../react/badge';

const meta: Meta = {
  title: 'Primitives/Card',
};

export default meta;
type Story = StoryObj;

export const Default: Story = {
  render: () => (
    <Card className="w-[380px]">
      <CardHeader>
        <CardTitle>airlock-hq/airlock</CardTitle>
        <CardDescription>Local-first Git proxy for clean PRs</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="flex items-center gap-2">
          <Badge variant="success">Enrolled</Badge>
          <span className="text-small text-foreground-muted">3 runs today</span>
        </div>
      </CardContent>
      <CardFooter>
        <Button variant="outline" size="sm">
          View Runs
        </Button>
      </CardFooter>
    </Card>
  ),
};

export const ErrorState: Story = {
  render: () => (
    <Card className="border-danger w-[380px]">
      <CardHeader>
        <CardTitle>Connection Error</CardTitle>
        <CardDescription>Unable to reach the Airlock daemon.</CardDescription>
      </CardHeader>
      <CardContent>
        <p className="text-small text-danger">
          The daemon is not running. Start it with <code className="bg-surface-elevated rounded px-1">make daemon</code>
          .
        </p>
      </CardContent>
    </Card>
  ),
};
