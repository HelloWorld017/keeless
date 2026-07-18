import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { useQueryState } from '@/fragments/_providers';
import { OpenFragment } from '@/fragments/open';
import { getRoute } from '@/utils/route';
import { AlertCircle, LoaderCircle } from 'lucide-react';
import { Redirect, Route, Switch } from 'wouter';

export const App = () => {
  const query = useQueryState();

  if (query.status === 'loading') {
    return (
      <main className="grid min-h-dvh place-items-center bg-[#090b10] text-zinc-400">
        <div className="flex items-center gap-3 text-sm">
          <LoaderCircle className="size-4 animate-spin text-indigo-300" />
          Starting the encrypted core...
        </div>
      </main>
    );
  }

  if (query.status === 'error') {
    return (
      <main className="grid min-h-dvh place-items-center bg-[#090b10] px-6 text-zinc-100">
        <Alert variant="destructive" className="max-w-md border-red-400/20 bg-red-400/8">
          <AlertCircle />
          <AlertTitle>Keeless could not start</AlertTitle>
          <AlertDescription>{query.error.message}</AlertDescription>
          <Button type="button" variant="outline" className="mt-3" onClick={query.retry}>
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
