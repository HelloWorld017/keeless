import { Button } from '@/components/button';
import { Input } from '@/components/input';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import { IconFile, IconLoaderCircle } from '@/icons';
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
  const [file, setFile] = useState<File>();
  const [sourcePassword, setSourcePassword] = useState('');
  const [currentPassword, setCurrentPassword] = useState('');
  const [pending, setPending] = useState<'export' | 'merge'>();
  const [error, setError] = useState<string>();
  const [summary, setSummary] = useState<string>();

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

  const download = async () => {
    setPending('export');
    setError(undefined);
    setSummary(undefined);
    try {
      const transfer = await requestClient.data!.request('prepareDatabaseExport', {
        password: currentPassword || null,
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
      setError(fileError(nextError, 'The database could not be exported.'));
    } finally {
      setPending(undefined);
    }
  };

  const merge = async () => {
    if (!file || !sourcePassword) {
      setError('Choose a database file and enter its master password.');
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
        password: currentPassword || null,
      });
      setSourcePassword('');
      setSummary(
        `Merged ${result.entriesAdded} added, ${result.entriesModified} modified, and ${result.entriesDeleted} deleted entries.`,
      );
      await Promise.all(
        refreshOperations.map(name =>
          queryClient.invalidateQueries({ queryKey: ['request', name] }),
        ),
      );
    } catch (nextError) {
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
        <Input
          type="password"
          autoComplete="current-password"
          placeholder="Dropped database password"
          value={sourcePassword}
          disabled={pending !== undefined}
          onChange={event => setSourcePassword(event.target.value)}
        />
        <Input
          type="password"
          autoComplete="current-password"
          placeholder="Current database password (when required)"
          value={currentPassword}
          disabled={pending !== undefined}
          onChange={event => setCurrentPassword(event.target.value)}
        />
        <Button
          type="button"
          disabled={pending !== undefined || !file || !sourcePassword}
          onClick={() => void merge()}
        >
          {pending === 'merge' && <IconLoaderCircle className="animate-spin" />}
          Merge database
        </Button>
      </div>

      {summary && <p className="text-sm text-muted-foreground">{summary}</p>}
      {error && (
        <p className="text-sm text-destructive" role="alert">
          {error}
        </p>
      )}
    </div>
  );
};
