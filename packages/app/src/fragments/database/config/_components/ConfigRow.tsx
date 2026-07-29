import type { ReactNode } from 'react';

export const ConfigRow = ({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) => (
  <div className="flex items-center gap-4 border-b py-4 last:border-b-0">
    <div className="min-w-0 flex-1">
      <h3 className="text-sm font-medium">{title}</h3>
      <p className="mt-0.5 text-sm text-muted-foreground">{description}</p>
    </div>
    <div className="shrink-0">{children}</div>
  </div>
);
