import { cn } from '@/utils/css';
import type { ComponentProps } from 'react';

function Tabs({ className, ...props }: ComponentProps<'div'>) {
  return <div data-slot="tabs" className={cn('w-full', className)} {...props} />;
}

function TabsList({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="tabs-list"
      role="tablist"
      className={cn(
        'grid h-8 grid-cols-3 rounded-lg bg-muted p-1 text-muted-foreground',
        className,
      )}
      {...props}
    />
  );
}

function TabsTrigger({
  className,
  active,
  ...props
}: ComponentProps<'button'> & { active: boolean }) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      data-state={active ? 'active' : 'inactive'}
      className={cn(
        'inline-flex items-center justify-center rounded-md px-2 text-sm font-medium outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring/50 data-[state=active]:bg-background data-[state=active]:text-foreground data-[state=active]:shadow-sm',
        className,
      )}
      {...props}
    />
  );
}

function TabsContent({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div data-slot="tabs-content" role="tabpanel" className={cn('mt-4', className)} {...props} />
  );
}

export { Tabs, TabsContent, TabsList, TabsTrigger };
