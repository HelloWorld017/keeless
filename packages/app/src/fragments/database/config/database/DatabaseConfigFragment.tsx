import { Button } from '@/components/button';
import { Input } from '@/components/input';
import { useHasNativePasswordInput } from '@/fragments/_providers/HostProvider';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import { PasswordPrompt } from '@/fragments/database/entryDetail/_components/PasswordPrompt';
import { IconFile, IconLoaderCircle } from '@/icons';
import { CoreRequestError } from '@/utils/request';
import { useQueryClient } from '@tanstack/react-query';
import { useState, type ChangeEvent, type DragEvent } from 'react';
import { ConfigRow } from '../_components';

const refreshOperations = [
  'getDatabaseStatus',
  'getEntries',
  'searchEntries',
  'getGroupHierarchy',
  'getGroupEntries',
  'getTagEntries',
  'getTrashEntries',
  'getTags',
  'getEntryDetail',
  'getCustomIcons',
  'getEntryTemplates',
] as const;

const fileError = (error: unknown, fallback: string) =>
  error instanceof Error && error.message ? error.message : fallback;

const needsPassword = (error: unknown) =>
  error instanceof CoreRequestError &&
  (error.code === 'password_required' || error.code === 'invalid_credentials');

const validFile = (file: File) => {
  if (!file.name.toLowerCase().endsWith('.kdbx')) {
    throw new Error('Choose a .kdbx database file.');
  }
  if (file.size === 0) {
    throw new Error('The database file is empty.');
  }
};

export const DatabaseConfigFragment = () => {
  const requestClient = useRequestClient();
  const queryClient = useQueryClient();
  const hasNativePasswordInput = useHasNativePasswordInput();
  const [file, setFile] = useState<File>();
  const [pending, setPending] = useState<'export' | 'merge'>();
  const [error, setError] = useState<string>();
  const [summary, setSummary] = useState<string>();
  const [passwordRequest, setPasswordRequest] = useState<
    | { type: 'source' }
    | { type: 'current-export'; invalid: boolean }
    | { type: 'current-merge'; sourcePassword: string; invalid: boolean }
  >();

  const selectFile = (nextFile: File | undefined) => {
    if (!nextFile) {
      return;
    }
    try {
      validFile(nextFile);
      setFile(nextFile);
      setError(undefined);
      setSummary(undefined);
    } catch (nextError) {
      setFile(undefined);
      setError(fileError(nextError, 'The database file could not be selected.'));
    }
  };

  const download = async (password?: string) => {
    setPending('export');
    setError(undefined);
    setSummary(undefined);
    try {
      const transfer = await requestClient.data!.request('prepareDatabaseExport', {
        password: password ?? null,
      });
      const bytes = await requestClient.data!.download(transfer.transferId);
      try {
        const url = URL.createObjectURL(new Blob([bytes], { type: 'application/octet-stream' }));
        const anchor = document.createElement('a');
        anchor.href = url;
        anchor.download = 'keeless-export.kdbx';
        anchor.click();
        setTimeout(() => URL.revokeObjectURL(url), 0);
      } finally {
        bytes.fill(0);
      }
    } catch (nextError) {
      if (!hasNativePasswordInput && needsPassword(nextError)) {
        setPasswordRequest({
          type: 'current-export',
          invalid:
            nextError instanceof CoreRequestError && nextError.code === 'invalid_credentials',
        });
        return;
      }
      setError(fileError(nextError, 'The database could not be exported.'));
    } finally {
      setPending(undefined);
    }
  };

  const merge = async (sourcePassword: string, password?: string) => {
    if (!file) {
      setError('Choose a database file.');
      return;
    }
    setPending('merge');
    setError(undefined);
    setSummary(undefined);
    try {
      const transferId = await requestClient.data!.upload(file);
      const result = await requestClient.data!.request('mergeTransferredDatabase', {
        transferId,
        sourcePassword,
        password: password ?? null,
      });
      setSummary(
        `Merged ${result.entriesAdded} added, ${result.entriesModified} modified, and ${result.entriesDeleted} deleted entries.`,
      );
      setError(result.syncError?.message);
      await Promise.all(
        refreshOperations.map(name =>
          queryClient.invalidateQueries({ queryKey: ['request', name] }),
        ),
      );
    } catch (nextError) {
      if (!hasNativePasswordInput && needsPassword(nextError)) {
        setPasswordRequest({
          type: 'current-merge',
          sourcePassword,
          invalid:
            nextError instanceof CoreRequestError && nextError.code === 'invalid_credentials',
        });
        return;
      }
      setError(fileError(nextError, 'The database could not be merged.'));
    } finally {
      setPending(undefined);
    }
  };

  const chooseFile = (event: ChangeEvent<HTMLInputElement>) => {
    selectFile(event.currentTarget.files?.[0]);
    event.currentTarget.value = '';
  };

  const dropFile = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    const files = [...event.dataTransfer.files];
    if (files.length !== 1) {
      setError('Drop one .kdbx database file.');
      return;
    }
    selectFile(files[0]);
  };

  return (
    <div className="space-y-4">
      <ConfigRow
        title="Export database"
        description="Download the current database as an encrypted KDBX file."
      >
        <Button type="button" disabled={pending !== undefined} onClick={() => void download()}>
          {pending === 'export' && <IconLoaderCircle className="animate-spin" />}
          Export
        </Button>
      </ConfigRow>

      <div className="space-y-3 border-b py-4">
        <div>
          <h3 className="text-sm font-medium">Merge database</h3>
          <p className="mt-0.5 text-sm text-muted-foreground">
            Drop another KDBX file to merge it into this database. Newer changes win conflicts.
          </p>
        </div>
        <div
          className="flex min-h-24 items-center gap-3 rounded-md border border-dashed p-4"
          onDragOver={event => {
            event.preventDefault();
            event.dataTransfer.dropEffect = 'copy';
          }}
          onDrop={dropFile}
        >
          <IconFile className="size-5 text-muted-foreground" />
          <div className="min-w-0 flex-1 text-sm">
            <p className="truncate font-medium">{file?.name ?? 'Drop one .kdbx file here'}</p>
            <p className="text-muted-foreground">
              {file ? `${file.size.toLocaleString()} bytes` : 'or choose a file'}
            </p>
          </div>
          <Input
            type="file"
            accept=".kdbx,application/octet-stream"
            disabled={pending !== undefined}
            className="max-w-36"
            onChange={chooseFile}
          />
        </div>
        <Button
          type="button"
          disabled={pending !== undefined || !file}
          variant={!file ? 'ghost' : 'default'}
          onClick={() => setPasswordRequest({ type: 'source' })}
        >
          {pending === 'merge' && <IconLoaderCircle className="animate-spin" />}
          Merge database
        </Button>
      </div>

      {(summary || error) && (
        <p className="text-sm text-muted-foreground flex flex-col" role="alert">
          {summary}
          {error && <span className="text-destructive">{error}</span>}
        </p>
      )}
      <PasswordPrompt
        open={passwordRequest !== undefined}
        pending={pending !== undefined}
        error={
          passwordRequest && 'invalid' in passwordRequest && passwordRequest.invalid
            ? 'The master password is incorrect.'
            : undefined
        }
        title={
          passwordRequest?.type === 'source' ? 'Unlock dropped database' : 'Confirm master password'
        }
        description={
          passwordRequest?.type === 'source'
            ? 'Enter the master password for the dropped database.'
            : 'Enter the master password for the current database.'
        }
        action={passwordRequest?.type === 'source' ? 'Merge database' : 'Continue'}
        onOpenChange={open => {
          if (!open && pending === undefined) {
            setPasswordRequest(undefined);
          }
        }}
        onSubmit={password => {
          const request = passwordRequest;
          setPasswordRequest(undefined);
          if (request?.type === 'source') {
            void merge(password);
          } else if (request?.type === 'current-export') {
            void download(password);
          } else if (request?.type === 'current-merge') {
            void merge(request.sourcePassword, password);
          }
        }}
      />
    </div>
  );
};
