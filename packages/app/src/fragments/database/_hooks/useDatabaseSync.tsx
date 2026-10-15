import { useHasNativePasswordInput } from '@/fragments/_providers/HostProvider';
import { useRequestMutation } from '@/fragments/_providers/QueryProvider';
import { CoreRequestError } from '@/utils/request';
import { useEffect, useRef, useState } from 'react';
import { PasswordPrompt } from '../_components/PasswordPrompt';
import { usePasswordRequest } from './usePasswordRequest';

type SyncOptions = { password?: string };
type SyncOutcome = { status: 'completed' | 'cancelled' };
type DatabaseAction = 'sync' | 'lock';
type PasswordRequest = {
  action: DatabaseAction;
  remoteKdfChanged: boolean;
  error?: string;
};

export const useDatabaseSync = () => {
  const hasNativePasswordInput = useHasNativePasswordInput();
  const saveDatabase = useRequestMutation('saveDatabase');
  const lockDatabase = useRequestMutation('lock');
  const activeOperation = useRef<symbol | undefined>(undefined);
  const [isPending, setIsPending] = useState(false);
  const passwordInput = usePasswordRequest<PasswordRequest>();
  const passwordRequest = passwordInput.request;
  const requestPending = saveDatabase.isPending || lockDatabase.isPending;

  useEffect(
    () => () => {
      activeOperation.current = undefined;
    },
    [],
  );

  const run = async (action: DatabaseAction, options: SyncOptions = {}): Promise<SyncOutcome> => {
    if (activeOperation.current !== undefined) {
      throw new Error('A database synchronization or lock is already in progress.');
    }
    const operation = Symbol();
    activeOperation.current = operation;
    const isCurrent = () => activeOperation.current === operation;
    let password = options.password;
    let remoteKdfChanged = false;
    setIsPending(true);

    try {
      while (isCurrent()) {
        try {
          const args = hasNativePasswordInput || password === undefined ? {} : { password };
          if (action === 'lock') {
            // Core synchronizes before locking; do not issue a separate save first.
            await lockDatabase.mutateAsync(args);
          } else {
            await saveDatabase.mutateAsync(args);
          }
          return { status: isCurrent() ? 'completed' : 'cancelled' };
        } catch (error) {
          if (!isCurrent()) {
            return { status: 'cancelled' };
          }
          if (!(error instanceof CoreRequestError)) {
            throw error;
          }
          if (hasNativePasswordInput) {
            if (error.code === 'password_required') {
              return { status: 'cancelled' };
            }
            throw error;
          }
          if (
            error.code !== 'password_required' &&
            error.code !== 'invalid_credentials' &&
            error.code !== 'database_credentials_required'
          ) {
            throw error;
          }

          remoteKdfChanged ||= error.code === 'database_credentials_required';
          const nextPassword = await passwordInput.ask({
            action,
            remoteKdfChanged,
            error:
              error.code === 'invalid_credentials'
                ? 'The master password is incorrect.'
                : undefined,
          });
          if (nextPassword === null || !isCurrent()) {
            return { status: 'cancelled' };
          }
          password = nextPassword;
        }
      }
      return { status: 'cancelled' };
    } finally {
      if (isCurrent()) {
        activeOperation.current = undefined;
        setIsPending(false);
      }
    }
  };

  const remoteKdfChanged = passwordRequest?.remoteKdfChanged === true;
  const locking = passwordRequest?.action === 'lock';

  return {
    sync: (options?: SyncOptions) => run('sync', options),
    lock: (options?: SyncOptions) => run('lock', options),
    isPending,
    prompt: (
      <PasswordPrompt
        open={passwordRequest !== undefined}
        pending={requestPending}
        error={passwordRequest?.error}
        title={
          remoteKdfChanged ? 'Reauthenticate database' : locking ? 'Lock database' : 'Sync database'
        }
        description={
          remoteKdfChanged
            ? 'The remote database’s encryption settings have changed. Enter its master password to synchronize your changes.'
            : 'Enter the master password to synchronize the database before continuing.'
        }
        action={
          remoteKdfChanged
            ? locking
              ? 'Authenticate and lock'
              : 'Authenticate and sync'
            : locking
              ? 'Lock'
              : 'Sync'
        }
        onOpenChange={open => {
          if (!open && !requestPending) {
            passwordInput.cancel();
          }
        }}
        onSubmit={password => {
          passwordInput.submit(password);
        }}
      />
    ),
  };
};
