import { HostProvider } from '@/fragments/_providers/HostProvider';
import { QueryProvider } from '@/fragments/_providers/QueryProvider';
import { RouterProvider } from '@/fragments/_providers/RouterProvider';
import { DatabaseFragment } from '@/fragments/database';
import { OpenFragment } from '@/fragments/open';
import { getRoute } from '@/utils/route';
import { StrictMode } from 'react';
import { Redirect, Route, Switch } from 'wouter';
import type { AppIntegration } from '@/types/AppIntegration';
import type { ReactNode } from 'react';

export const App = () => (
  <Switch>
    <Route path={getRoute('open')} component={OpenFragment} />
    <Route path={getRoute('database')} component={DatabaseFragment} />
    <Redirect to={getRoute('open')} replace />
  </Switch>
);

export const AppFrame = ({
  children,
  integration,
}: {
  children: ReactNode;
  integration: AppIntegration;
}) => (
  <StrictMode>
    <HostProvider integration={integration}>
      <QueryProvider>
        <RouterProvider fallback="open">{children}</RouterProvider>
      </QueryProvider>
    </HostProvider>
  </StrictMode>
);
