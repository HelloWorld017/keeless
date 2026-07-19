import { Alert, AlertDescription, AlertTitle } from '@/components/alert';
import { Button } from '@/components/button';
import { Input } from '@/components/input';
import { Item, ItemContent, ItemDescription, ItemMedia, ItemTitle } from '@/components/item';
import { StepError } from '@/fragments/open/_components/StepError';
import { IconChevronLeft, IconFile, IconLoaderCircle } from '@/icons';
import { useState } from 'react';
import type { StorageSetupComponentProps } from '@/types/Host';
import type { BrowserCore } from '@keeless/host-browser';
import type { ChangeEvent, DragEvent } from 'react';

const pickerOptions: OpenFilePickerOptions = {
  multiple: false,
  excludeAcceptAllOption: false,
  types: [
    {
      description: 'KeePass database',
      accept: { 'application/octet-stream': ['.kdbx'] },
    },
  ],
};

const fileError = (error: unknown) =>
  error instanceof Error ? error.message : 'The selected file could not be opened.';

const validateFile = (file: File) => {
  if (!file.name.toLowerCase().endsWith('.kdbx')) {
    throw new Error('Choose a .kdbx database file.');
  }
};

const requestWritableHandle = async (handle: FileSystemFileHandle) => {
  try {
    const current = await handle.queryPermission?.({ mode: 'readwrite' });
    if (current === 'granted') {
      return handle;
    }
    if (!handle.requestPermission) {
      return handle;
    }
    return (await handle.requestPermission({ mode: 'readwrite' })) === 'granted'
      ? handle
      : undefined;
  } catch {
    return undefined;
  }
};

export const LocalFileSetup = ({
  getCore,
  isPending,
  error,
  onBack,
  onOpen,
}: StorageSetupComponentProps & { getCore: () => BrowserCore }) => {
  const [isAcquiring, setIsAcquiring] = useState(false);
  const [localError, setLocalError] = useState<string>();
  const supportsPicker =
    typeof window !== 'undefined' && typeof window.showOpenFilePicker === 'function';
  const pending = isPending || isAcquiring;

  const openFile = async (file: File, handle?: FileSystemFileHandle) => {
    validateFile(file);
    await onOpen(async () => {
      await getCore().configureLocalFile(file, handle);
      return { provider: 'local-file', path: '' };
    });
  };

  const openHandle = async (handle: FileSystemFileHandle) => {
    const writableHandle = await requestWritableHandle(handle);
    await openFile(await handle.getFile(), writableHandle);
  };

  const runAcquisition = async (acquire: () => Promise<void>) => {
    setIsAcquiring(true);
    setLocalError(undefined);
    try {
      await acquire();
    } catch (nextError) {
      if (!(nextError instanceof DOMException && nextError.name === 'AbortError')) {
        setLocalError(fileError(nextError));
      }
    } finally {
      setIsAcquiring(false);
    }
  };

  const pickFile = () => {
    if (!window.showOpenFilePicker) {
      return;
    }
    void runAcquisition(async () => {
      const [handle] = await window.showOpenFilePicker!(pickerOptions);
      if (!handle) {
        return;
      }
      await openHandle(handle);
    });
  };

  const selectFile = (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.currentTarget.files?.[0];
    event.currentTarget.value = '';
    if (file) {
      void runAcquisition(() => openFile(file));
    }
  };

  const dropFile = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    const items = [...event.dataTransfer.items].filter(item => item.kind === 'file');
    if (items.length !== 1) {
      setLocalError('Drop one .kdbx database file.');
      return;
    }
    const item = items[0];
    const fallbackFile = item.getAsFile();
    const handlePromise = item.getAsFileSystemHandle?.();
    void runAcquisition(async () => {
      const handle = await handlePromise?.catch(() => null);
      if (handle?.kind === 'file') {
        await openHandle(handle as FileSystemFileHandle);
        return;
      }
      if (handle?.kind === 'directory') {
        throw new Error('Drop a .kdbx file instead of a folder.');
      }
      if (!fallbackFile) {
        throw new Error('The dropped file could not be read.');
      }
      await openFile(fallbackFile);
    });
  };

  return (
    <div className="space-y-4">
      <Item
        variant="outline"
        className="min-h-28"
        onDragOver={event => {
          event.preventDefault();
          event.dataTransfer.dropEffect = 'copy';
        }}
        onDrop={dropFile}
      >
        <ItemMedia variant="icon">
          <IconFile />
        </ItemMedia>
        <ItemContent>
          <ItemTitle>Drop a database file</ItemTitle>
          <ItemDescription>Drop one .kdbx file here or choose it from this device.</ItemDescription>
        </ItemContent>
      </Item>

      {supportsPicker ? (
        <Button type="button" className="w-full" disabled={pending} onClick={pickFile}>
          {pending && <IconLoaderCircle className="animate-spin" />}
          Choose file
        </Button>
      ) : (
        <div className="space-y-2">
          <Input
            type="file"
            accept=".kdbx,application/octet-stream"
            disabled={pending}
            onChange={selectFile}
          />
          <Alert>
            <AlertTitle>Read-only file</AlertTitle>
            <AlertDescription>
              This browser can open the selected database but cannot write changes back to it.
            </AlertDescription>
          </Alert>
        </div>
      )}

      <StepError error={localError ?? error} />
      <Button type="button" variant="outline" disabled={pending} onClick={onBack}>
        <IconChevronLeft /> Back
      </Button>
    </div>
  );
};
