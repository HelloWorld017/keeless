import { Button } from '@/components/button';
import { useHistoryBack } from '@/fragments/_providers/RouterProvider';
import { IconArrowLeft } from '@/icons';
import type { ReactNode } from 'react';

export const SetupLayout = ({
  title,
  description,
  children,
  showBack = true,
}: {
  title: string;
  description: string;
  children: ReactNode;
  showBack?: boolean;
}) => {
  const historyBack = useHistoryBack();

  return (
    <main className="flex flex-col items-center justify-center">
      <div className="flex w-full max-w-120 flex-col px-10 py-10 rounded-xl">
        {showBack && (
          <Button
            type="button"
            variant="ghost"
            className="self-start -ml-2"
            onClick={() => historyBack()}
          >
            <IconArrowLeft /> Back
          </Button>
        )}
        <h1 className={showBack ? 'mt-8 text-3xl font-bold' : 'text-3xl font-bold'}>{title}</h1>
        <span className="text-muted-foreground mt-2">{description}</span>
        <div className="w-full mt-12 space-y-5">{children}</div>
      </div>
    </main>
  );
};
