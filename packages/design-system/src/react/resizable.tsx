'use client';

import type { ComponentProps } from 'react';
import { Panel, PanelGroup, PanelResizeHandle } from 'react-resizable-panels';

import { cn } from '../utils/cn';

function ResizablePanelGroup({ className, ...props }: ComponentProps<typeof PanelGroup>) {
  return <PanelGroup className={cn('flex h-full w-full', className)} {...props} />;
}

const ResizablePanel = Panel;

function ResizableHandle({ className, ...props }: ComponentProps<typeof PanelResizeHandle>) {
  return (
    <PanelResizeHandle
      className={cn(
        'bg-border-subtle relative z-10 w-px shrink-0 cursor-col-resize',
        'hover:bg-border',
        'active:bg-signal/50',
        'after:absolute after:inset-y-0 after:-right-1 after:-left-1 after:content-[""]',
        className
      )}
      {...props}
    />
  );
}

export { ResizablePanelGroup, ResizablePanel, ResizableHandle };
