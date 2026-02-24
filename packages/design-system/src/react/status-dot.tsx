import * as React from 'react';
import { cva, type VariantProps } from 'class-variance-authority';

import { cn } from '../utils/cn';

const statusDotVariants = cva('shrink-0 rounded-full', {
  variants: {
    variant: {
      success: 'bg-success',
      danger: 'bg-danger',
      warning: 'bg-warning',
      signal: 'bg-signal',
      muted: 'bg-foreground-muted',
    },
    size: {
      sm: 'h-1.5 w-1.5',
      md: 'h-2 w-2',
    },
    pulse: {
      true: 'animate-pulse',
      false: '',
    },
  },
  defaultVariants: {
    variant: 'muted',
    size: 'sm',
    pulse: false,
  },
});

export interface StatusDotProps extends React.HTMLAttributes<HTMLDivElement>, VariantProps<typeof statusDotVariants> {}

function StatusDot({ className, variant, size, pulse, ...props }: StatusDotProps) {
  return <div className={cn(statusDotVariants({ variant, size, pulse }), className)} {...props} />;
}

export { StatusDot, statusDotVariants };
