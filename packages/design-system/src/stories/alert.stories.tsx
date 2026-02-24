import type { Meta, StoryObj } from '@storybook/react';
import { AlertCircle, CheckCircle2, AlertTriangle, Info } from 'lucide-react';
import { Alert, AlertTitle, AlertDescription } from '../react/alert';

const meta: Meta = {
  title: 'Primitives/Alert',
};

export default meta;
type Story = StoryObj;

export const Default: Story = {
  render: () => (
    <Alert>
      <Info className="h-4 w-4" />
      <AlertTitle>Information</AlertTitle>
      <AlertDescription>Pipeline is configured and ready to run.</AlertDescription>
    </Alert>
  ),
};

export const Warning: Story = {
  render: () => (
    <Alert variant="warning">
      <AlertTriangle className="h-4 w-4" />
      <AlertTitle>Awaiting Approval</AlertTitle>
      <AlertDescription>Stage "create-pr" requires manual approval before continuing.</AlertDescription>
    </Alert>
  ),
};

export const Danger: Story = {
  render: () => (
    <Alert variant="danger">
      <AlertCircle className="h-4 w-4" />
      <AlertTitle>Pipeline Failed</AlertTitle>
      <AlertDescription>The lint stage exited with code 1. Check the logs for details.</AlertDescription>
    </Alert>
  ),
};

export const AllVariants: Story = {
  render: () => (
    <div className="flex w-[480px] flex-col gap-4">
      <Alert>
        <Info className="h-4 w-4" />
        <AlertTitle>Default</AlertTitle>
        <AlertDescription>Neutral informational alert.</AlertDescription>
      </Alert>
      <Alert variant="warning">
        <AlertTriangle className="h-4 w-4" />
        <AlertTitle>Warning</AlertTitle>
        <AlertDescription>Something needs attention.</AlertDescription>
      </Alert>
      <Alert variant="danger">
        <AlertCircle className="h-4 w-4" />
        <AlertTitle>Danger</AlertTitle>
        <AlertDescription>Something went wrong.</AlertDescription>
      </Alert>
      <Alert variant="default">
        <CheckCircle2 className="h-4 w-4" />
        <AlertTitle>Success pattern</AlertTitle>
        <AlertDescription>Use default variant with a success icon for positive states.</AlertDescription>
      </Alert>
    </div>
  ),
};
