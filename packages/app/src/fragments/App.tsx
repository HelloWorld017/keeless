import { ToastList } from '@/fragments/_components/ToastList';
import { HostProvider } from '@/fragments/_providers/HostProvider';
import { QueryProvider } from '@/fragments/_providers/QueryProvider';
import { RouterProvider } from '@/fragments/_providers/RouterProvider';
import { ToastProvider } from '@/fragments/_providers/ToastProvider';
import { DatabaseFragment } from '@/fragments/database';
import { OpenFragment } from '@/fragments/open';
import { getRoute } from '@/utils/route';
import { StrictMode } from 'react';
import { Redirect, Route, Switch } from 'wouter';
import { EntryFocusHandler } from './_components/EntryFocusHandler';
import type { AppIntegration } from '@/types/AppIntegration';

export const AppContents = () => (
  <Switch>
    <Route path={getRoute('open')} component={OpenFragment} />
    <Route path={getRoute('search')} component={DatabaseFragment} />
    <Route path={getRoute('group')} component={DatabaseFragment} />
    <Route path={getRoute('tag')} component={DatabaseFragment} />
    <Route path={getRoute('trash')} component={DatabaseFragment} />
    <Route path={getRoute('database')} component={DatabaseFragment} />
    <Redirect to={getRoute('open')} replace />
  </Switch>
);

type AppProps = {
  integration: AppIntegration;
};

export const App = ({ integration }: AppProps) => (
  <StrictMode>
    <HostProvider integration={integration}>
      <QueryProvider>
        <ToastProvider>
          <RouterProvider fallback="open">
            <AppContents />
            <EntryFocusHandler />
          </RouterProvider>
          <ToastList />
        </ToastProvider>
      </QueryProvider>
    </HostProvider>
  </StrictMode>
);
