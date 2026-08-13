import { useState } from 'react';
import type { StorageSetupComponentProps } from '@keeless/app';

type PickMode = 'open' | 'create';

const messageFor = (error: unknown) =>
  error instanceof Error ? error.message : 'The file picker failed.';

export const LocalFileSetup = ({ isPending, error, onOpen }: StorageSetupComponentProps) => {
  const [isPicking, setIsPicking] = useState(false);
  const [pickerError, setPickerError] = useState<string>();
  const pending = isPending || isPicking;

  const pick = async (mode: PickMode) => {
    setIsPicking(true);
    setPickerError(undefined);
    try {
      const capabilityToken = await window.keelessDesktop.pickLocalFile(mode);
      if (capabilityToken) {
        await onOpen(() => ({ provider: 'local-file', path: capabilityToken }));
      }
    } catch (nextError) {
      setPickerError(messageFor(nextError));
    } finally {
      setIsPicking(false);
    }
  };

  return (
    <div className="space-y-4">
      <button
        type="button"
        className="flex w-full items-center gap-3 rounded-lg border p-4 text-left transition-colors hover:bg-accent disabled:opacity-50"
        disabled={pending}
        onClick={() => void pick('open')}
      >
        <span className="font-medium">Open existing database</span>
      </button>
      <button
        type="button"
        className="flex w-full items-center gap-3 rounded-lg border p-4 text-left transition-colors hover:bg-accent disabled:opacity-50"
        disabled={pending}
        onClick={() => void pick('create')}
      >
        <span className="font-medium">Create new database</span>
      </button>
      {(pickerError ?? error) && (
        <p role="alert" className="text-sm text-destructive">
          {pickerError ?? error}
        </p>
      )}
    </div>
  );
};
