import type { ReactNode } from 'react';

export const SetupLayout = ({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) => (
  <main className="flex flex-col items-center justify-center">
    <div className="flex w-full max-w-120 flex-col px-10 py-10 rounded-xl">
      <h1 className="text-3xl font-bold">{title}</h1>
      <span className="text-muted-foreground mt-2">{description}</span>
      <div className="w-full mt-12 space-y-5">{children}</div>
    </div>
  </main>
);
