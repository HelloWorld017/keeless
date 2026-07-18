import { Alert, AlertDescription, AlertTitle } from '@/components/alert';
import { Button } from '@/components/button';
import { QueryProvider, useRequestClient } from '@/fragments/_providers/QueryProvider';
import { RouterProvider } from '@/fragments/_providers/RouterProvider';
import { OpenFragment } from '@/fragments/open';
import { IconAlertCircle, IconLoaderCircle } from '@/icons';
import { getRoute } from '@/utils/route';
import { ReactNode, StrictMode } from 'react';
import { Redirect, Route, Switch } from 'wouter';

export const App = () => {
  const requestClient = useRequestClient();

  if (requestClient.isPending) {
    return (
      <main className="grid min-h-dvh place-items-center">
        <div className="flex items-center gap-3 text-sm">
          <IconLoaderCircle className="animate-spin text-blue-300" />
          Starting the core...
        </div>
      </main>
    );
  }

  if (requestClient.isError) {
    return (
      <main className="grid min-h-dvh place-items-center px-6">
        <Alert variant="destructive">
          <IconAlertCircle />
          <AlertTitle>Keeless could not start</AlertTitle>
          <AlertDescription>{requestClient.error.message}</AlertDescription>
          <Button
            type="button"
            variant="outline"
            className="mt-3"
            onClick={() => void requestClient.refetch()}
          >
            Try again
          </Button>
        </Alert>
      </main>
    );
  }

  return (
    <Switch>
      <Route path={getRoute('open')} component={OpenFragment} />
      <Route path={getRoute('database')} component={OpenFragment} />
      <Redirect to={getRoute('open')} replace />
    </Switch>
  );
};

export const AppFrame = ({ children }: { children: ReactNode }) => (
  <StrictMode>
    <QueryProvider>
      <RouterProvider fallback="open">{children}</RouterProvider>
    </QueryProvider>
  </StrictMode>
);
