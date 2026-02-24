import * as React from 'react';
import { cva, type VariantProps } from 'class-variance-authority';

import { cn } from '../utils/cn';

const badgeVariants = cva(
  'inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-semibold transition-colors focus:outline-hidden focus:ring-2 focus:ring-ring focus:ring-offset-2',
  {
    variants: {
      variant: {
        default: 'border-border-subtle bg-surface text-foreground hover:bg-surface-elevated',
        signal: 'border-transparent bg-signal-subtle text-signal hover:bg-signal-subtle/70',
        secondary: 'border-transparent bg-surface-elevated text-foreground hover:bg-surface',
        danger: 'border-transparent bg-danger/10 text-danger hover:bg-danger/20',
        destructive: 'border-transparent bg-danger/10 text-danger hover:bg-danger/20',
        outline: 'text-foreground',
        success: 'border-transparent bg-success/10 text-success hover:bg-success/20',
        warning: 'border-transparent bg-warning/10 text-warning hover:bg-warning/20',
      },
    },
    defaultVariants: {
      variant: 'default',
    },
  }
);

export interface BadgeProps extends React.HTMLAttributes<HTMLDivElement>, VariantProps<typeof badgeVariants> {}

function Badge({ className, variant, ...props }: BadgeProps) {
  return <div className={cn(badgeVariants({ variant }), className)} {...props} />;
}

export { Badge, badgeVariants };
