import { Button } from '@/components/button';
import { usePasswordInput } from '@/fragments/_providers/HostProvider';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import { IconEye, IconEyeOff, IconLoaderCircle } from '@/icons';
import { CoreRequestError } from '@/utils/request';
import { useEffect, useRef, useState } from 'react';
import { PasswordPrompt } from './PasswordPrompt';
import type { DatabaseNodeId } from '@keeless/schema';

const revealError = (error: unknown) =>
  error instanceof CoreRequestError && error.code === 'invalid_credentials'
    ? 'The master password is incorrect.'
    : 'The protected value could not be revealed.';

export const FieldPassword = ({
  entryId,
  fieldIndex,
  name,
  onReveal,
  compact = false,
}: {
  entryId: DatabaseNodeId;
  fieldIndex: number;
  name: string;
  onReveal?: (value: string) => void;
  compact?: boolean;
}) => {
  const requestClient = useRequestClient();
  const onPasswordInput = usePasswordInput();
  const [value, setValue] = useState<string>();
  const [pending, setPending] = useState(false);
  const [promptOpen, setPromptOpen] = useState(false);
  const [error, setError] = useState<string>();
  const operationRef = useRef(0);
  const revealed = value !== undefined;

  useEffect(
    () => () => {
      operationRef.current += 1;
    },
    [],
  );

  const reveal = (password?: string) =>
    requestClient.data!.request('revealEntryField', { entryId, fieldIndex, password });
  const acceptValue = (nextValue: string, operation: number) => {
    if (operation !== operationRef.current) {
      return;
    }
    setValue(nextValue);
    onReveal?.(nextValue);
  };
  const show = async () => {
    const operation = ++operationRef.current;
    setPending(true);
    setError(undefined);
    try {
      acceptValue((await reveal()).value, operation);
    } catch (nextError) {
      if (operation !== operationRef.current) {
        return;
      }
      if (nextError instanceof CoreRequestError && nextError.code === 'password_required') {
        if (onPasswordInput) {
          const password = await onPasswordInput('reveal');
          if (password && operation === operationRef.current) {
            try {
              acceptValue((await reveal(password)).value, operation);
            } catch (passwordError) {
              if (operation === operationRef.current) {
                setError(revealError(passwordError));
              }
            }
          }
        } else {
          setPromptOpen(true);
        }
      } else {
        setError(revealError(nextError));
      }
    } finally {
      if (operation === operationRef.current) {
        setPending(false);
      }
    }
  };
  const submitPassword = async (password: string) => {
    const operation = ++operationRef.current;
    setPending(true);
    setError(undefined);
    try {
      acceptValue((await reveal(password)).value, operation);
      if (operation === operationRef.current) {
        setPromptOpen(false);
      }
    } catch (nextError) {
      if (operation === operationRef.current) {
        setError(revealError(nextError));
      }
    } finally {
      if (operation === operationRef.current) {
        setPending(false);
      }
    }
  };

  const revealButton = (
    <Button
      type="button"
      variant={compact ? 'outline' : 'ghost'}
      size={compact ? 'sm' : 'icon-sm'}
      className={compact ? undefined : '-my-1 shrink-0 text-muted-foreground'}
      aria-label={revealed ? `Hide ${name}` : `Reveal ${name}`}
      aria-pressed={revealed}
      disabled={pending}
      onClick={() => {
        if (revealed) {
          operationRef.current += 1;
          setValue(undefined);
          setError(undefined);
        } else {
          void show();
        }
      }}
    >
      {pending ? (
        <IconLoaderCircle className="animate-spin" />
      ) : revealed ? (
        <IconEyeOff />
      ) : (
        <IconEye />
      )}
      {compact && (revealed ? 'Hide value' : 'Reveal existing value')}
    </Button>
  );
  const prompt = (
    <PasswordPrompt
      open={promptOpen}
      pending={pending}
      error={error}
      title="Reveal protected value"
      description="Enter the master password for this database."
      action="Reveal"
      onOpenChange={open => {
        setPromptOpen(open);
        if (!open) {
          operationRef.current += 1;
          setPending(false);
          setError(undefined);
        }
      }}
      onSubmit={password => void submitPassword(password)}
    />
  );

  if (compact) {
    return (
      <div className="space-y-1">
        {revealButton}
        {error && !promptOpen && (
          <p className="text-xs text-destructive" role="alert">
            {error}
          </p>
        )}
        {prompt}
      </div>
    );
  }

  return (
    <div className="space-y-1 px-4 py-3">
      <dt className="text-xs text-muted-foreground">{name || 'Untitled field'}</dt>
      <dd className="flex min-w-0 items-start gap-2 text-sm">
        <span
          className={`min-w-0 flex-1 break-words ${revealed && !value ? 'text-muted-foreground' : ''}`}
        >
          {revealed ? (
            value || 'Empty'
          ) : (
            <>
              <span className="tracking-[0.2em]" aria-hidden="true">
                ●●●●●●●●●●●
              </span>
              <span className="sr-only">Protected value</span>
            </>
          )}
        </span>
        {revealButton}
      </dd>
      {error && !promptOpen && (
        <p className="text-xs text-destructive" role="alert">
          {error}
        </p>
      )}
      {prompt}
    </div>
  );
};
