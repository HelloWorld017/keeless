import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/alert-dialog';
import { Button } from '@/components/button';
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemMedia,
  ItemTitle,
} from '@/components/item';
import {
  useRequest,
  useRequestClient,
  useRequestMutation,
} from '@/fragments/_providers/QueryProvider';
import { useNavigate } from '@/fragments/_providers/RouterProvider';
import { IconArrowRight, IconDatabase, IconLoaderCircle, IconPlus, IconTrash } from '@/icons';
import { buildRoute } from '@/utils/route';
import { useMemo, useState } from 'react';
import { SetupLayout } from '../_components/SetupLayout';
import { StepError } from '../_components/StepError';
import { errorMessage } from '../_utils/errorMessage';
import type { OperationResult } from '@/utils/request';
import {formatDate} from '@/utils/format';
import {useHost} from '@/fragments/_providers/HostProvider';

type RecentDatabase = OperationResult<'getRecentDatabases'>['databases'][number];

export const RecentStep = () => {
  const navigate = useNavigate();
  const requestClient = useRequestClient();
  const recentDatabases = useRequest('getRecentDatabases', {});
  const open = useRequestMutation('open');
  const deleteRecentDatabase = useRequestMutation('deleteRecentDatabase');
  const [databaseToDelete, setDatabaseToDelete] = useState<RecentDatabase>();
  const [error, setError] = useState<string>();

  const openDatabase = async (id: string) => {
    if (!requestClient.data || open.isPending) {
      return;
    }

    setError(undefined);
    try {
      await open.mutateAsync({ databaseId: id });
      const { database } = await requestClient.data.request('getCoreStatus', {});
      if (database === 'unlocked') {
        await requestClient.data.upgrade();
        navigate(buildRoute('database'), { replace: true });
      } else {
        navigate(buildRoute(database === 'locked' ? 'openUnlock' : 'openCreate'));
      }
    } catch (nextError) {
      setError(errorMessage(nextError));
    }
  };

  const deleteDatabase = async () => {
    if (!databaseToDelete) {
      return;
    }

    setError(undefined);
    try {
      await deleteRecentDatabase.mutateAsync({ id: databaseToDelete.id });
      setDatabaseToDelete(undefined);
    } catch (nextError) {
      setError(errorMessage(nextError));
    }
  };

  const host = useHost();
  const storageNames = useMemo(() => new Map(host?.storages.map(storage => [storage.kind, storage.label])), [host]);

  return (
    <SetupLayout
      title="Open a database"
      description="Choose a recently opened database."
      showBack={false}
    >
      <ItemGroup className="gap-2">
        {recentDatabases.data?.databases.slice(0, 5).map(database => (
          <Item key={database.id} variant="outline">
            <ItemMedia variant="icon">
              <IconDatabase />
            </ItemMedia>
            <ItemContent className="gap-0">
              <ItemTitle className="font-semibold">{database.name}</ItemTitle>
              <ItemDescription className='flex flex-col sm:block'>
                <span>{storageNames.get(database.storageType) ?? database.storageType}</span>
                <span className='hidden size-[3px] rounded-full mx-3 bg-muted-foreground/50 align-[calc(0.5cap-1.5px)] sm:inline-block' />
                <time dateTime={new Date(database.lastOpenedAtMs).toISOString()}>
                  {formatDate(database.lastOpenedAtMs)}
                </time>
              </ItemDescription>
            </ItemContent>
            <ItemActions>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                aria-label={`Delete ${database.name}`}
                disabled={open.isPending || deleteRecentDatabase.isPending}
                onClick={() => setDatabaseToDelete(database)}
              >
                <IconTrash />
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                aria-label={`Open ${database.name}`}
                disabled={open.isPending || deleteRecentDatabase.isPending}
                onClick={() => void openDatabase(database.id)}
              >
                {open.isPending ? (
                  <IconLoaderCircle className="animate-spin" />
                ) : (
                  <IconArrowRight />
                )}
              </Button>
            </ItemActions>
          </Item>
        ))}
        <Item
          variant="outline"
          render={<button type="button" className="transition-colors hover:bg-muted" />}
          onClick={() => navigate(buildRoute('openSelect'))}
        >
          <ItemMedia variant="icon">
            <IconPlus />
          </ItemMedia>
          <ItemContent>
            <ItemTitle className="font-semibold">Add database</ItemTitle>
          </ItemContent>
        </Item>
      </ItemGroup>
      <StepError
        error={error ?? (recentDatabases.isError ? errorMessage(recentDatabases.error) : undefined)}
      />

      <AlertDialog
        open={Boolean(databaseToDelete)}
        onOpenChange={open =>
          !deleteRecentDatabase.isPending && !open && setDatabaseToDelete(undefined)
        }
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete recent database?</AlertDialogTitle>
            <AlertDialogDescription>
              {databaseToDelete?.name} will be removed from the recent databases list. The database
              itself will not be deleted.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleteRecentDatabase.isPending}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              disabled={deleteRecentDatabase.isPending}
              onClick={() => void deleteDatabase()}
            >
              {deleteRecentDatabase.isPending && <IconLoaderCircle className="animate-spin" />}
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </SetupLayout>
  );
};
