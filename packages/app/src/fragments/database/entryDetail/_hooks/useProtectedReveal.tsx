import { usePasswordInput } from '@/fragments/_providers/HostProvider';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import { CoreRequestError } from '@/utils/request';
import { useEffect, useRef, useState } from 'react';
import { PasswordPrompt } from '../_components/PasswordPrompt';
import type { DatabaseNodeId } from '@keeless/schema';

type UseProtectedRevealOptions = {
  entryId: DatabaseNodeId;
  fieldId: string | null;
  onReveal: (value: string | undefined) => void;
};

const revealError = (error: unknown) =>
  error instanceof CoreRequestError && error.code === 'invalid_credentials'
    ? 'The master password is incorrect.'
    : 'The protected value could not be revealed.';

export const useProtectedReveal = ({ entryId, fieldId, onReveal }: UseProtectedRevealOptions) => {
  const [pending, setPending] = useState(false);
  const [promptOpen, setPromptOpen] = useState(false);
  const [error, setError] = useState<string>();
  const [revealed, setRevealed] = useState(false);
  const operationRef = useRef(0);
  useEffect(
    () => () => {
      operationRef.current += 1;
    },
    [],
  );

  const requestClient = useRequestClient();
  const onPasswordInput = usePasswordInput();

  if (fieldId === null) {
    return {
      prompt: <></>,
      promptOpen: false,
      pending: false,
      error: undefined,
      revealed,
      toggleReveal: () => setRevealed(value => !value),
    };
  }

  const reveal = (password?: string) =>
    requestClient.data!.request('revealEntryField', { entryId, fieldId, password });

  const toggleReveal = async () => {
    if (revealed) {
      operationRef.current += 1;
      onReveal(undefined);
      setRevealed(false);
      setError(undefined);
      return;
    }

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

  const acceptValue = (nextValue: string, operation: number) => {
    if (operation !== operationRef.current) {
      return;
    }
    onReveal(nextValue);
    setRevealed(true);
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

  return {
    prompt,
    promptOpen,
    pending,
    error,
    revealed,
    toggleReveal,
  };
};
