import type { Meta, StoryObj } from '@storybook/react';
import {
  Dialog,
  DialogTrigger,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
  DialogDescription,
} from '../react/dialog';
import { Button } from '../react/button';
import { Input } from '../react/input';
import { Label } from '../react/label';

const meta: Meta = {
  title: 'Primitives/Dialog',
  parameters: { layout: 'centered' },
};

export default meta;
type Story = StoryObj;

export const Default: Story = {
  render: () => (
    <Dialog>
      <DialogTrigger asChild>
        <Button variant="outline">Open Dialog</Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Enroll Repository</DialogTitle>
          <DialogDescription>Add a repository to the Airlock pipeline.</DialogDescription>
        </DialogHeader>
        <div className="grid gap-4 py-4">
          <div className="grid gap-1.5">
            <Label htmlFor="url">Repository URL</Label>
            <Input id="url" placeholder="https://github.com/..." />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline">Cancel</Button>
          <Button variant="signal">Enroll</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  ),
};

export const Danger: Story = {
  render: () => (
    <Dialog>
      <DialogTrigger asChild>
        <Button variant="danger">Delete</Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Confirm Deletion</DialogTitle>
          <DialogDescription>
            This action cannot be undone. The repository will be permanently removed.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline">Cancel</Button>
          <Button variant="danger">Delete Repository</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  ),
};
