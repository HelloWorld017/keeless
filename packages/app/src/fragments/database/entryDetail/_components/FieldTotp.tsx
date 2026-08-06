import { useHasNativePasswordInput } from '@/fragments/_providers/HostProvider';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import { CoreRequestError } from '@/utils/request';
import { useEffect, useEffectEvent, useRef, useState } from 'react';
import { FieldCopyButton } from './FieldCopyButton';
import { PasswordPrompt } from './PasswordPrompt';
import type { DatabaseNodeId, GetEntryTotpResult } from '@keeless/schema';

export const FieldTotp = ({
  entryId,
  fieldId,
  name,
}: {
  entryId: DatabaseNodeId;
  fieldId?: string;
  name: string;
}) => {
  const [now, setNow] = useState(Date.now());
  const [result, setResult] = useState<GetEntryTotpResult>();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string>();
  const [passwordOpen, setPasswordOpen] = useState(false);
  const [passwordError, setPasswordError] = useState<string>();
  const requestClient = useRequestClient();
  const hasNativePasswordInput = useHasNativePasswordInput();
  const operationRef = useRef(0);
  const remaining = result ? Math.max(0, Math.ceil((result.expiresAtMs - now) / 1000)) : 0;
  const progress = result ? Math.min(100, (remaining / result.period) * 100) : 0;

  const load = useEffectEvent(async (password?: string) => {
    if (!requestClient.data) {
      return;
    }
    const operation = ++operationRef.current;
    setPending(true);
    setError(undefined);
    try {
      const next = await requestClient.data.request(
        'getEntryTotp',
        hasNativePasswordInput
          ? { entryId, ...(fieldId ? { fieldId } : {}) }
          : { entryId, ...(fieldId ? { fieldId } : {}), password },
      );
      if (operation === operationRef.current) {
        setResult(next);
        setPasswordOpen(false);
        setPasswordError(undefined);
      }
    } catch (nextError) {
      if (operation !== operationRef.current) {
        return;
      }
      if (nextError instanceof CoreRequestError && nextError.code === 'password_required') {
        if (!hasNativePasswordInput) {
          setPasswordOpen(true);
        }
      } else if (
        nextError instanceof CoreRequestError &&
        nextError.code === 'invalid_credentials'
      ) {
        setPasswordOpen(true);
        setPasswordError('The master password is incorrect.');
      } else {
        setError('The current OTP could not be displayed.');
      }
    } finally {
      if (operation === operationRef.current) {
        setPending(false);
      }
    }
  });

  useEffect(() => {
    const interval = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(interval);
  }, []);

  useEffect(() => {
    void load();
    return () => {
      operationRef.current += 1;
    };
  }, [entryId, fieldId, requestClient.data]);

  useEffect(() => {
    if (result && now >= result.expiresAtMs && !pending) {
      void load();
    }
  }, [now, pending, result]);

  return (
    <div className="space-y-2 px-4 py-3">
      <dt className="text-xs text-muted-foreground">{name}</dt>
      <dd className="flex min-w-0 items-center gap-2">
        <div className="min-w-0 flex-1">
          <p className="font-mono text-xl font-semibold tracking-[0.18em]">
            {result?.code ?? (pending ? '......' : 'Unavailable')}
          </p>
          {result && (
            <div className="mt-1 flex items-center gap-2 text-xs text-muted-foreground">
              <div className="h-1 flex-1 overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full bg-primary transition-[width]"
                  style={{ width: `${progress}%` }}
                />
              </div>
              <span>{remaining}s</span>
            </div>
          )}
        </div>
        {result && <FieldCopyButton label={name} value={result.code} />}
      </dd>
      {error && <p className="text-xs text-destructive">{error}</p>}
      <PasswordPrompt
        open={passwordOpen}
        pending={pending}
        error={passwordError}
        title="Reveal OTP"
        description="Enter the master password for this database."
        action="Reveal"
        onOpenChange={open => {
          if (!open) {
            operationRef.current += 1;
            setPasswordOpen(false);
            setPasswordError(undefined);
          }
        }}
        onSubmit={password => void load(password)}
      />
    </div>
  );
};
