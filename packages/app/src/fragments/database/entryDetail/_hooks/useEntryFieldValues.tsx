import { useHasNativePasswordInput } from '@/fragments/_providers/HostProvider';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import { useShowToast } from '@/fragments/_providers/ToastProvider';
import { useLatestRef } from '@/hooks/useLatestRef';
import { CoreRequestError } from '@/utils/request';
import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from 'react';
import { PasswordPrompt } from '../_components/PasswordPrompt';
import type { DatabaseNodeId } from '@keeless/schema';

type RevealAction = {
  fieldIds: string[];
  reveal: boolean;
  copy?: { fieldId: string; label: string };
};

type PendingAction = {
  action: RevealAction;
  operation: number;
};

type EntryFieldValues = {
  values: Record<string, string>;
  pendingFieldIds: Set<string>;
  hideAll: () => void;
  revealAll: (fieldIds: string[]) => void;
  toggleReveal: (fieldId: string) => void;
  copyProtected: (fieldId: string, label: string) => void;
  copyPublic: (value: string, label: string) => void;
};

const EntryFieldValuesContext = createContext<EntryFieldValues | null>(null);

const fieldError = (error: unknown) =>
  error instanceof CoreRequestError && error.code === 'invalid_credentials'
    ? 'The master password is incorrect.'
    : 'The protected value could not be revealed.';

export const useEntryFieldValues = (entryId: DatabaseNodeId) => {
  const requestClient = useRequestClient();
  const hasNativePasswordInput = useHasNativePasswordInput();
  const showToast = useShowToast();
  const [values, setValues] = useState<Record<string, string>>({});
  const [pendingFieldIds, setPendingFieldIds] = useState(new Set<string>());
  const [pendingAction, setPendingAction] = useState<PendingAction>();
  const [passwordError, setPasswordError] = useState<string>();
  const valuesRef = useLatestRef(values);
  const operationRef = useRef(0);

  useEffect(() => {
    operationRef.current += 1;
    setValues({});
    setPendingFieldIds(new Set());
    setPendingAction(undefined);
    setPasswordError(undefined);
    return () => {
      operationRef.current += 1;
    };
  }, [entryId]);

  const copyValue = async (value: string, label: string) => {
    try {
      await navigator.clipboard.writeText(value);
      showToast({ message: `${label || 'Field'} copied.`, durationMs: 3000 });
    } catch {
      showToast({
        kind: 'destructive',
        message: `${label || 'Field'} could not be copied.`,
      });
    }
  };

  const execute = async (action: RevealAction, operation: number, password?: string) => {
    if (!requestClient.data) {
      return;
    }

    const loaded = Object.fromEntries(
      action.fieldIds.flatMap(fieldId => {
        const value = valuesRef.current[fieldId];
        return value === undefined ? [] : [[fieldId, value]];
      }),
    );

    const fieldIds = action.fieldIds.filter(fieldId => valuesRef.current[fieldId] === undefined);
    if (fieldIds.length > 0) {
      const { values: revealedValues } = await requestClient.data.request(
        'revealEntryFields',
        hasNativePasswordInput ? { entryId, fieldIds } : { entryId, fieldIds, password },
      );

      fieldIds.forEach((fieldId, index) => {
        loaded[fieldId] = revealedValues[index];
      });
    }

    if (operation !== operationRef.current) {
      return;
    }

    if (action.reveal) {
      const next = { ...valuesRef.current, ...loaded };
      valuesRef.current = next;
      setValues(next);
    }

    if (action.copy) {
      await copyValue(loaded[action.copy.fieldId], action.copy.label);
    }
  };

  const run = async (action: RevealAction) => {
    const operation = ++operationRef.current;
    setPendingFieldIds(new Set(action.fieldIds));
    setPasswordError(undefined);
    try {
      await execute(action, operation);
    } catch (error) {
      if (operation !== operationRef.current) {
        return;
      }
      if (error instanceof CoreRequestError && error.code === 'password_required') {
        if (!hasNativePasswordInput) {
          setPendingAction({ action, operation });
        }
      } else {
        showToast({ kind: 'destructive', message: fieldError(error) });
      }
    } finally {
      if (operation === operationRef.current) {
        setPendingFieldIds(new Set());
      }
    }
  };

  const submitPassword = async (password: string) => {
    if (!pendingAction) {
      return;
    }
    const { action, operation } = pendingAction;
    setPendingFieldIds(new Set(action.fieldIds));
    setPasswordError(undefined);
    try {
      await execute(action, operation, password);
      if (operation === operationRef.current) {
        setPendingAction(undefined);
      }
    } catch (error) {
      if (operation === operationRef.current) {
        setPasswordError(fieldError(error));
      }
    } finally {
      if (operation === operationRef.current) {
        setPendingFieldIds(new Set());
      }
    }
  };

  const controller: EntryFieldValues = {
    values,
    pendingFieldIds,
    hideAll: () => {
      operationRef.current += 1;
      valuesRef.current = {};
      setValues({});
      setPendingFieldIds(new Set());
      setPendingAction(undefined);
      setPasswordError(undefined);
    },
    revealAll: fieldIds => {
      if (fieldIds.length > 0) {
        void run({ fieldIds, reveal: true });
      }
    },
    toggleReveal: fieldId => {
      if (valuesRef.current[fieldId] !== undefined) {
        const next = { ...valuesRef.current };
        delete next[fieldId];
        valuesRef.current = next;
        setValues(next);
      } else {
        void run({ fieldIds: [fieldId], reveal: true });
      }
    },
    copyProtected: (fieldId, label) =>
      void run({ fieldIds: [fieldId], reveal: false, copy: { fieldId, label } }),
    copyPublic: (value, label) => void copyValue(value, label),
  };

  return {
    controller,
    prompt: (
      <PasswordPrompt
        open={Boolean(pendingAction)}
        pending={pendingFieldIds.size > 0}
        error={passwordError}
        title="Reveal protected fields"
        description="Enter the master password for this database."
        action={pendingAction?.action.copy ? 'Copy' : 'Reveal'}
        onOpenChange={open => {
          if (!open) {
            operationRef.current += 1;
            setPendingAction(undefined);
            setPendingFieldIds(new Set());
            setPasswordError(undefined);
          }
        }}
        onSubmit={password => void submitPassword(password)}
      />
    ),
  };
};

export const EntryFieldValuesProvider = ({
  value,
  children,
}: {
  value: EntryFieldValues;
  children: ReactNode;
}) => <EntryFieldValuesContext value={value}>{children}</EntryFieldValuesContext>;

export const useEntryFieldValueActions = () => {
  const value = useContext(EntryFieldValuesContext);
  if (!value) {
    throw new Error('EntryFieldValuesProvider is missing.');
  }
  return value;
};
