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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/dropdown-menu';
import { Skeleton } from '@/components/skeleton';
import { useHasNativePasswordInput } from '@/fragments/_providers/HostProvider';
import { useRequest, useRequestClient } from '@/fragments/_providers/QueryProvider';
import { useShowToast } from '@/fragments/_providers/ToastProvider';
import { useDebouncedValue } from '@/hooks/useDebouncedValue';
import {
  IconAlertCircle,
  IconChevronLeft,
  IconEllipsisVertical,
  IconEye,
  IconLoaderCircle,
  IconPencil,
  IconTrash,
} from '@/icons';
import { cx } from '@/utils/css';
import { CoreRequestError } from '@/utils/request';
import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { getEntryTitle } from '../_components/EntryItem';
import { IconPicker } from '../_components/IconPicker';
import { ItemIcon } from '../_components/ItemIcon';
import { EditContent } from './_components/EditContent';
import { PasswordPrompt } from './_components/PasswordPrompt';
import { ViewContent } from './_components/ViewContent';
import { useEntryEditor } from './_hooks/useEntryEditor';
import { EntryFieldValuesProvider, useEntryFieldValues } from './_hooks/useEntryFieldValues';
import { usePasswordConfirmations } from './_hooks/usePasswordConfirmations';
import type { EntryAttachmentUpdate, EntrySummary } from '@keeless/schema';

const REFRESH_OPERATIONS = [
  'getEntries',
  'searchEntries',
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

const EntryDetailSkeleton = ({ pending }: { pending: boolean }) => {
  const visible = useDebouncedValue(pending, 150, false);
  if (!visible) {
    return null;
  }
  return (
    <div
      className={cx(
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
  const hasNativePasswordInput = useHasNativePasswordInput();
  const showToast = useShowToast();
  const mountedRef = useRef(true);
  const operationRef = useRef(0);
  const passwordRequestRef = useRef<PasswordRequest | undefined>(undefined);
  const [editing, setEditing] = useState(false);
  const [attachments, setAttachments] = useState<File[]>([]);
  const [removedAttachmentIndices, setRemovedAttachmentIndices] = useState<number[]>([]);
  const editor = useEntryEditor();
  const fieldValues = useEntryFieldValues(entry.id);
  const confirmations = usePasswordConfirmations();
  const [error, setError] = useState<string>();
  const [pending, setPending] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [passwordRequest, setPasswordRequest] = useState<PasswordRequest>();
  const protectedFieldIds =
    detail.data?.fields.flatMap(field =>
      field.type === 'field' && field.fieldId !== null && field.isProtected ? [field.fieldId] : [],
    ) ?? [];

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
        if (hasNativePasswordInput) {
          return null;
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
        nextPassword =>
          hasNativePasswordInput
            ? requestClient.data!.request('saveDatabase', {})
            : requestClient.data!.request('saveDatabase', { password: nextPassword }),
        operation,
        password,
      );

      if (!saved) {
        if (hasNativePasswordInput) {
          return;
        }
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

    editor.begin(detail.data);
    setAttachments([]);
    setRemovedAttachmentIndices([]);
    fieldValues.controller.hideAll();
    confirmations.clear();
    setError(undefined);
    setEditing(true);
  };

  const clearEditing = () => {
    setEditing(false);
    setAttachments([]);
    setRemovedAttachmentIndices([]);
    editor.clear();
    confirmations.clear();
    setError(undefined);
  };

  const submit = async () => {
    if (!detail.data) {
      return;
    }
    const fieldsValid = editor.validate();
    const confirmationsValid = confirmations.validate(detail.data.fields, editor.drafts);
    if (!fieldsValid || !confirmationsValid) {
      return;
    }

    const operation = ++operationRef.current;
    const fields = editor.fieldUpdates();
    const propertiesUpdate = editor.propertiesUpdate(detail.data);

    setPending(true);
    setError(undefined);

    try {
      const attachmentUpdates: EntryAttachmentUpdate[] = [];
      for (const attachment of attachments) {
        attachmentUpdates.push({
          transferId: await requestClient.data!.upload(attachment),
          name: attachment.name,
        });
      }
      const updated = await requestWithPassword(
        password =>
          hasNativePasswordInput
            ? requestClient.data!.request('updateEntry', {
                entryId: entry.id,
                fields,
                properties: propertiesUpdate,
                attachments: attachmentUpdates,
                removedAttachmentIndices,
              })
            : requestClient.data!.request('updateEntry', {
                entryId: entry.id,
                fields,
                properties: propertiesUpdate,
                attachments: attachmentUpdates,
                removedAttachmentIndices,
                password,
              }),
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
          <div className="flex gap-1 justify-end">
            {editing && (
              <>
                <Button
                  type="button"
                  size="lg"
                  variant="outline"
                  disabled={pending}
                  onClick={clearEditing}
                >
                  Cancel
                </Button>
                <Button type="button" size="lg" disabled={pending} onClick={() => void submit()}>
                  {pending ? <IconLoaderCircle className="animate-spin" /> : 'Done'}
                </Button>
              </>
            )}
            {!editing && detail.data && (
              <>
                <Button variant="ghost" size="lg" onClick={startEditing}>
                  <IconPencil />
                  Edit
                </Button>
                <DropdownMenu>
                  <DropdownMenuTrigger
                    render={<Button variant="ghost" size="icon-lg" aria-label="Entry options" />}
                  >
                    <IconEllipsisVertical />
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="min-w-44">
                    <DropdownMenuItem
                      disabled={
                        protectedFieldIds.length === 0 ||
                        fieldValues.controller.pendingFieldIds.size > 0
                      }
                      onClick={() => fieldValues.controller.revealAll(protectedFieldIds)}
                    >
                      <IconEye />
                      Reveal all fields
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem variant="destructive" onClick={() => setDeleteOpen(true)}>
                      <IconTrash />
                      {inTrash ? 'Delete permanently' : 'Move to Trash'}
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </>
            )}
            {!detail.data && <div className="h-9" />}
          </div>
        </div>
        <div className="flex min-w-0 flex-1 items-center gap-3 text-xl md:text-2xl 2xl:justify-center lg:text-3xl">
          {editing ? (
            <IconPicker
              value={editor.properties.icon}
              fallback="entry"
              disabled={pending}
              onChange={icon => editor.changeProperties({ icon })}
              iconClassName="size-7.5"
              render={
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-lg"
                  className="shrink-0 size-11.5 -m-2 text-muted-foreground"
                />
              }
            />
          ) : (
            <ItemIcon
              icon={detail.data?.icon ?? entry.icon}
              fallback="entry"
              className="shrink-0 text-muted-foreground"
            />
          )}
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
                isTemplate={detail.data?.isTemplate ?? false}
                drafts={editor.drafts}
                fields={detail.data?.fields ?? []}
                properties={editor.properties}
                confirmations={confirmations.values}
                confirmationErrors={confirmations.errors}
                attachments={attachments}
                existingAttachments={(detail.data?.attachments ?? []).filter(
                  attachment => !removedAttachmentIndices.includes(attachment.index),
                )}
                errors={editor.errors}
                pending={pending}
                onLoad={editor.load}
                onAdd={editor.add}
                onChange={editor.change}
                onDelete={editor.remove}
                onPropertiesChange={editor.changeProperties}
                onConfirmationChange={confirmations.change}
                onAttachmentsChange={setAttachments}
                onExistingAttachmentDelete={attachmentIndex =>
                  setRemovedAttachmentIndices(indices => [...indices, attachmentIndex])
                }
              />
            ) : (
              <EntryFieldValuesProvider value={fieldValues.controller}>
                <ViewContent detail={detail.data} />
              </EntryFieldValuesProvider>
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
      {fieldValues.prompt}
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
    className={cx(
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
