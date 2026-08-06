import { cn } from '@/utils/css';
import type { ComponentProps } from 'react';

function Empty({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="empty"
      className={cn(
        'flex min-h-52 flex-col items-center justify-center gap-3 rounded-lg border border-dashed p-6 text-center text-muted-foreground',
        className,
      )}
      {...props}
    />
  );
}

function EmptyTitle({ className, ...props }: ComponentProps<'p'>) {
  return (
    <p
      data-slot="empty-title"
      className={cn('font-medium text-foreground', className)}
      {...props}
    />
  );
}

function EmptyDescription({ className, ...props }: ComponentProps<'p'>) {
  return (
    <p data-slot="empty-description" className={cn('max-w-sm text-sm', className)} {...props} />
  );
}

export { Empty, EmptyDescription, EmptyTitle };
