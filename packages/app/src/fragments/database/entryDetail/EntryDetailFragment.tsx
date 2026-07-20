import { Alert, AlertAction, AlertDescription, AlertTitle } from '@/components/alert';
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
import { Skeleton } from '@/components/skeleton';
import { usePasswordInput } from '@/fragments/_providers/HostProvider';
import { useRequest, useRequestClient } from '@/fragments/_providers/QueryProvider';
import { useShowToast } from '@/fragments/_providers/ToastProvider';
import { useDebouncedValue } from '@/hooks/useDebouncedValue';
import { IconAlertCircle, IconChevronLeft, IconLoaderCircle, IconPencil, IconTrash } from '@/icons';
import { cn } from '@/utils/css';
import { CoreRequestError } from '@/utils/request';
import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { getEntryTitle } from '../_components/EntryItem';
import { ItemIcon } from '../_components/ItemIcon';
import { EditContent } from './_components/EditContent';
import { PasswordPrompt } from './_components/PasswordPrompt';
import { ViewContent } from './_components/ViewContent';
import { STANDARD_NAMES } from './_constants';
import { FieldDraft } from './_types/FieldDraft';
import type { EntryDetailResult, EntryFieldUpdate, EntrySummary } from '@keeless/schema';

const REFRESH_OPERATIONS = [
  'getEntries',
  'getGroupEntries',
  'getTagEntries',
  'getTrashEntries',
  'getEntryTemplates',
  'getTags',
  'getEntryDetail',
] as const;

type PasswordRequest = {
  error?: string;
  resolve: (password: string | null) => void;
};

const operationError = (error: unknown, fallback: string) =>
  error instanceof CoreRequestError && error.message ? error.message : fallback;

const needsPassword = (error: unknown) =>
  error instanceof CoreRequestError &&
  (error.code === 'password_required' || error.code === 'invalid_credentials');

const createDrafts = (detail: EntryDetailResult): FieldDraft[] => {
  const fields = new Map(detail.fields.map(field => [field.fieldIndex, field]));
  const standard = STANDARD_NAMES.map((name, fieldIndex) => {
    const field = fields.get(fieldIndex);
    return {
      key: `standard-${fieldIndex}`,
      fieldIndex,
      name,
      value: field?.isProtected ? null : (field?.value ?? ''),
      isProtected: field?.isProtected ?? fieldIndex === STANDARD_NAMES.indexOf('Password'),
      valueChanged: !field,
    };
  });

  const custom = detail.fields
    .filter(field => field.fieldIndex >= STANDARD_NAMES.length)
    .map(field => ({
      key: `custom-${field.fieldIndex}`,
      fieldIndex: field.fieldIndex,
      name: field.name,
      value: field.isProtected ? null : (field.value ?? ''),
      isProtected: field.isProtected,
      valueChanged: false,
    }));
  return [...standard, ...custom];
};

const EntryDetailSkeleton = ({ pending }: { pending: boolean }) => {
  const visible = useDebouncedValue(pending, 150, false);
  if (!visible) {
    return null;
  }
  return (
    <div
      className={cn(
        'animate-in w-full max-w-3xl space-y-8 p-6 duration-200 fade-in',
        !pending && 'animate-out opacity-0 fade-out',
      )}
      aria-label="Loading entry details"
    >
      <div className="space-y-3">
        <Skeleton className="h-5 w-20" />
        <Skeleton className="h-20 w-full" />
        <Skeleton className="h-20 w-full" />
        <Skeleton className="h-20 w-full" />
      </div>
      <div className="space-y-3">
        <Skeleton className="h-5 w-24" />
        <Skeleton className="h-32 w-full" />
      </div>
    </div>
  );
};

const EntryDetailQuery = ({
  entry,
  inTrash,
  onClose,
}: {
  entry: EntrySummary;
  inTrash: boolean;
  onClose: () => void;
}) => {
  const detail = useRequest('getEntryDetail', { entryId: entry.id });
  const requestClient = useRequestClient();
  const queryClient = useQueryClient();
  const onPasswordInput = usePasswordInput();
  const showToast = useShowToast();
  const nextKey = useRef(0);
  const mountedRef = useRef(true);
  const operationRef = useRef(0);
  const passwordRequestRef = useRef<PasswordRequest | undefined>(undefined);
  const [editing, setEditing] = useState(false);
  const [drafts, setDrafts] = useState<FieldDraft[]>([]);
  const [fieldErrors, setFieldErrors] = useState(new Set<string>());
  const [error, setError] = useState<string>();
  const [pending, setPending] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [passwordRequest, setPasswordRequest] = useState<PasswordRequest>();

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      operationRef.current += 1;
      passwordRequestRef.current?.resolve(null);
      passwordRequestRef.current = undefined;
    };
  }, []);

  const refresh = async (removeDetail = false) => {
    if (removeDetail) {
      queryClient.removeQueries({ queryKey: ['request', 'getEntryDetail', { entryId: entry.id }] });
    }
    await Promise.all(
      REFRESH_OPERATIONS.filter(name => !removeDetail || name !== 'getEntryDetail').map(name =>
        queryClient.invalidateQueries({ queryKey: ['request', name] }),
      ),
    );
  };

  const isCurrentOperation = (operation: number) =>
    mountedRef.current && operation === operationRef.current;

  const askPassword = async (invalid: boolean, operation: number) => {
    if (!isCurrentOperation(operation)) {
      return null;
    }

    if (onPasswordInput) {
      const password = await onPasswordInput('save');
      return isCurrentOperation(operation) ? password : null;
    }

    return new Promise<string | null>(resolve => {
      if (!isCurrentOperation(operation)) {
        resolve(null);
        return;
      }

      const request = {
        error: invalid ? 'The master password is incorrect.' : undefined,
        resolve,
      };

      passwordRequestRef.current = request;
      setPasswordRequest(request);
    });
  };

  const requestWithPassword = async <T,>(
    operation: (password?: string) => Promise<T>,
    operationToken: number,
    initialPassword?: string,
  ) => {
    let password = initialPassword;
    let attempted = initialPassword !== undefined;
    while (true) {
      try {
        return { result: await operation(password), password };
      } catch (nextError) {
        if (!needsPassword(nextError)) {
          throw nextError;
        }

        const nextPassword = await askPassword(
          attempted ||
            (nextError instanceof CoreRequestError && nextError.code === 'invalid_credentials'),
          operationToken,
        );

        if (!nextPassword) {
          return null;
        }

        password = nextPassword;
        attempted = true;
      }
    }
  };

  const saveAfterMutation = async (message: string, operation: number, password?: string) => {
    try {
      const saved = await requestWithPassword(
        nextPassword => requestClient.data!.request('saveDatabase', { password: nextPassword }),
        operation,
        password,
      );

      if (!saved) {
        throw new Error('Password entry was cancelled.');
      }
    } catch {
      showToast({
        kind: 'destructive',
        message,
      });
    }
  };

  const startEditing = () => {
    if (!detail.data) {
      return;
    }

    setDrafts(createDrafts(detail.data));
    setFieldErrors(new Set());
    setError(undefined);
    setEditing(true);
  };

  const clearEditing = () => {
    setEditing(false);
    setDrafts([]);
    setFieldErrors(new Set());
    setError(undefined);
  };

  const submit = async () => {
    const invalid = new Set(
      drafts
        .filter(field => field.fieldIndex === null || field.fieldIndex >= STANDARD_NAMES.length)
        .filter(field => !field.name.trim())
        .map(field => field.key),
    );
    setFieldErrors(invalid);

    if (invalid.size) {
      return;
    }

    const operation = ++operationRef.current;
    const fields: EntryFieldUpdate[] = drafts.map(field => ({
      fieldIndex: field.fieldIndex,
      name:
        field.fieldIndex !== null && field.fieldIndex < STANDARD_NAMES.length
          ? STANDARD_NAMES[field.fieldIndex]
          : field.name.trim(),
      value: field.isProtected && !field.valueChanged ? null : field.value,
      isProtected: field.isProtected,
    }));

    setPending(true);
    setError(undefined);

    try {
      const updated = await requestWithPassword(
        password =>
          requestClient.data!.request('updateEntry', { entryId: entry.id, fields, password }),
        operation,
      );

      if (!updated) {
        return;
      }

      if (isCurrentOperation(operation)) {
        clearEditing();
      }

      await refresh();
      await saveAfterMutation(
        'The changes could not be saved to storage. Changes remain in memory.',
        operation,
        updated.password,
      );
      await refresh();
    } catch (nextError) {
      if (isCurrentOperation(operation)) {
        setError(operationError(nextError, 'The entry could not be updated. Try again.'));
      }
    } finally {
      if (isCurrentOperation(operation)) {
        setPending(false);
      }
    }
  };

  const deleteEntry = async () => {
    const operation = ++operationRef.current;
    setDeleteOpen(false);
    setPending(true);
    setError(undefined);
    try {
      await requestClient.data!.request('deleteEntry', { entryId: entry.id, permanent: inTrash });
      await saveAfterMutation(
        'The deletion could not be saved to storage. The deletion remains in memory.',
        operation,
      );

      if (isCurrentOperation(operation)) {
        onClose();
      }

      await refresh(true);
    } catch (nextError) {
      if (isCurrentOperation(operation)) {
        setError(
          operationError(
            nextError,
            `The entry could not be ${inTrash ? 'deleted' : 'moved to Trash'}. Try again.`,
          ),
        );
      }
    } finally {
      if (isCurrentOperation(operation)) {
        setPending(false);
      }
    }
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-center gap-2 xl:flex-col xl:items-stretch px-4 pb-2 pt-4 md:px-6 md:pb-6 xl:pt-0">
        <Button
          variant="ghost"
          size="icon-lg"
          className="md:hidden"
          aria-label="Back to entries"
          disabled={pending || editing}
          onClick={onClose}
        >
          <IconChevronLeft />
        </Button>
        <div className="order-2 xl:pt-4 xl:order-none">
          {editing && (
            <div className="flex gap-1 justify-end">
              <Button type="button" size="lg" variant="outline" disabled={pending} onClick={clearEditing}>
                Cancel
              </Button>
              <Button type="button" size="lg" disabled={pending} onClick={() => void submit()}>
                {pending ? <IconLoaderCircle className="animate-spin" /> : 'Done'}
              </Button>
            </div>
          )}
          {!editing && detail.data && (
            <div className="flex gap-1 justify-end">
              <Button
                variant="ghost"
                size="lg"
                onClick={startEditing}
              >
                <IconPencil />
                Edit
              </Button>
              <Button
                className="text-destructive hover:text-destructive hover:bg-destructive/10"
                variant="ghost"
                size="icon-lg"
                onClick={() => setDeleteOpen(true)}
              >
                <IconTrash />
              </Button>
            </div>
          )}
        </div>
        <div className="flex min-w-0 flex-1 items-center gap-3 text-xl md:text-2xl 2xl:justify-center lg:text-3xl">
          <ItemIcon
            icon={entry.icon}
            fallback="entry"
            className="hidden shrink-0 text-muted-foreground sm:block"
          />
          <h1 id="entry-detail-title" className="min-w-0 truncate font-semibold">
            {getEntryTitle(entry)}
          </h1>
        </div>
      </header>
      <div className="mx-auto w-full max-w-180 min-h-0 overflow-auto">
        {detail.isPending ? (
          <EntryDetailSkeleton pending />
        ) : detail.isError ? (
          <Alert variant="destructive" className="m-4 w-auto max-w-3xl sm:m-6">
            <IconAlertCircle />
            <AlertTitle>Entry details could not be loaded</AlertTitle>
            <AlertDescription>Try loading the entry again.</AlertDescription>
            <AlertAction>
              <Button variant="outline" size="sm" onClick={() => detail.refetch()}>
                Retry
              </Button>
            </AlertAction>
          </Alert>
        ) : (
          <>
            {error && (
              <Alert
                variant="destructive"
                className="m-4 mb-0 w-auto max-w-3xl sm:mx-6"
                aria-label="Entry operation error"
              >
                <IconAlertCircle />
                <AlertTitle>Entry operation failed</AlertTitle>
                <AlertDescription>{error}</AlertDescription>
              </Alert>
            )}
            {editing ? (
              <EditContent
                entryId={entry.id}
                drafts={drafts}
                errors={fieldErrors}
                pending={pending}
                onAdd={() =>
                  setDrafts(current => [
                    ...current,
                    {
                      key: `new-${nextKey.current++}`,
                      fieldIndex: null,
                      name: '',
                      value: '',
                      isProtected: false,
                      valueChanged: true,
                    },
                  ])
                }
                onChange={(key, patch) => {
                  setDrafts(current =>
                    current.map(field => (field.key === key ? { ...field, ...patch } : field)),
                  );

                  if ('name' in patch) {
                    setFieldErrors(current => {
                      const next = new Set(current);
                      next.delete(key);
                      return next;
                    });
                  }
                }}
                onDelete={key => setDrafts(current => current.filter(field => field.key !== key))}
              />
            ) : (
              <ViewContent detail={detail.data} />
            )}
          </>
        )}
      </div>
      <AlertDialog open={deleteOpen} onOpenChange={open => !pending && setDeleteOpen(open)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {inTrash ? 'Delete entry permanently?' : 'Move entry to Trash?'}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {inTrash
                ? `"${getEntryTitle(entry)}" will be permanently deleted. This cannot be undone.`
                : `"${getEntryTitle(entry)}" will be moved to Trash and can be restored later.`}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={pending}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              disabled={pending}
              onClick={() => void deleteEntry()}
            >
              {inTrash ? 'Delete permanently' : 'Move to Trash'}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <PasswordPrompt
        open={Boolean(passwordRequest)}
        pending={false}
        error={passwordRequest?.error}
        title="Save database"
        description="Enter the master password to save these changes."
        action="Continue"
        onOpenChange={open => {
          if (!open && passwordRequest) {
            passwordRequest.resolve(null);
            if (passwordRequestRef.current === passwordRequest) {
              passwordRequestRef.current = undefined;
            }
            setPasswordRequest(undefined);
          }
        }}
        onSubmit={password => {
          passwordRequest?.resolve(password);
          if (passwordRequestRef.current === passwordRequest) {
            passwordRequestRef.current = undefined;
          }
          setPasswordRequest(undefined);
        }}
      />
      <div className="sr-only" aria-live="polite">
        {pending ? 'Entry operation in progress' : ''}
      </div>
    </div>
  );
};

export const EntryDetailFragment = ({
  entry,
  selected,
  listPending,
  inTrash,
  onClose,
}: {
  entry?: EntrySummary;
  selected: boolean;
  listPending: boolean;
  inTrash: boolean;
  onClose: () => void;
}) => (
  <section
    className={cn(
      'min-h-0 min-w-0 flex-3 flex-col p-4 md:pt-2 xl:p-6 xl:pt-2 xl:pb-8',
      selected ? 'flex' : 'hidden md:flex',
    )}
    aria-labelledby={selected ? 'entry-detail-title' : undefined}
  >
    {selected ? (
      entry ? (
        <EntryDetailQuery
          key={String(entry.id)}
          entry={entry}
          inTrash={inTrash}
          onClose={onClose}
        />
      ) : listPending ? (
        <EntryDetailSkeleton pending />
      ) : (
        <p className="p-6 text-center text-sm text-muted-foreground">Entry does not exist</p>
      )
    ) : (
      <div className="flex flex-1 items-center justify-center p-6">
        <p className="text-sm text-muted-foreground">Select an entry to view its details</p>
      </div>
    )}
  </section>
);
