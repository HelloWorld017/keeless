import { useEffect, useState } from 'react';
import type { PasskeyCheck, PasskeyState } from '@/types/DesktopBridge';

const errorMessage = (error: unknown) =>
  error instanceof Error ? error.message : 'Passkey status could not be loaded.';

const statePresentation = (state: PasskeyState['state']) => {
  switch (state) {
    case 'enabled':
      return {
        label: 'Enabled',
        className: 'border-transparent bg-primary text-primary-foreground',
      };
    case 'disabled':
      return {
        label: 'Disabled',
        className: 'border-transparent bg-secondary text-secondary-foreground',
      };
    case 'degraded':
      return {
        label: 'Needs attention',
        className: 'border-transparent bg-destructive/10 text-destructive',
      };
    case 'unsupported':
      return {
        label: 'Unsupported',
        className: 'border-transparent bg-destructive/10 text-destructive',
      };
  }
  return {
    label: 'Unsupported',
    className: 'border-transparent bg-destructive/10 text-destructive',
  };
};

const checkPresentation = (check: PasskeyCheck) => {
  switch (check.status) {
    case 'ok':
      return { label: 'Ready', className: 'text-emerald-600 dark:text-emerald-400' };
    case 'warning':
      return { label: 'Inactive', className: 'text-muted-foreground' };
    case 'error':
      return { label: 'Error', className: 'text-destructive' };
  }
  return { label: 'Error', className: 'text-destructive' };
};

export const PasskeyConfig = () => {
  const [state, setState] = useState<PasskeyState>();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string>();

  const refresh = async () => {
    try {
      setState(await window.keelessDesktop.getPasskeyState());
      setError(undefined);
    } catch (nextError) {
      setError(errorMessage(nextError));
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const setEnabled = async (enabled: boolean) => {
    setPending(true);
    setError(undefined);
    try {
      setState(await window.keelessDesktop.setPasskeyEnabled(enabled));
    } catch (nextError) {
      // Preserve the last known state when polkit is dismissed or an action fails.
      setError(errorMessage(nextError));
    } finally {
      setPending(false);
    }
  };

  const presentation = state && statePresentation(state.state);
  const canToggle = state && state.state !== 'unsupported';

  return (
    <div className="flex flex-col gap-4 overflow-hidden rounded-xl bg-card py-4 text-sm text-card-foreground ring-1 ring-foreground/10">
      <div className="grid grid-cols-[1fr_auto] gap-1 px-4">
        <div>
          <h3 className="font-heading text-base leading-snug font-medium">Passkey</h3>
          <p className="text-sm text-muted-foreground">Use Keeless as a system passkey provider.</p>
        </div>
        {presentation && (
          <span
            className={`inline-flex h-fit items-center justify-center rounded-md border px-2 py-0.5 text-xs font-medium whitespace-nowrap ${presentation.className}`}
          >
            {presentation.label}
          </span>
        )}
      </div>
      <div className="px-4">
        {!state && !error && <p className="text-sm text-muted-foreground">Loading status...</p>}
        {state && (
          <div className="space-y-3">
            {state.checks.map(check => {
              const status = checkPresentation(check);
              return (
                <div key={check.id} className="flex items-start justify-between gap-4">
                  <div className="min-w-0">
                    <p className="font-medium">{check.label}</p>
                    {check.detail && (
                      <p className="text-sm text-muted-foreground">{check.detail}</p>
                    )}
                  </div>
                  <span className={`shrink-0 text-sm font-medium ${status.className}`}>
                    {status.label}
                  </span>
                </div>
              );
            })}
          </div>
        )}
        {error && (
          <p className="mt-4 text-sm text-destructive" role="alert">
            {error}
          </p>
        )}
      </div>
      <div className="flex items-center justify-end gap-2 rounded-b-xl border-t bg-muted/50 p-4">
        {error && !pending && !state && (
          <button
            type="button"
            className="h-8 rounded-lg border border-border bg-background px-2.5 text-sm font-medium hover:bg-muted disabled:pointer-events-none disabled:opacity-50"
            onClick={() => void refresh()}
          >
            Retry
          </button>
        )}
        {canToggle && (
          <button
            type="button"
            className={`h-8 rounded-lg px-2.5 text-sm font-medium disabled:pointer-events-none disabled:opacity-50 ${state.enabled ? 'bg-destructive/10 text-destructive hover:bg-destructive/20' : 'bg-primary text-primary-foreground hover:bg-primary/80'}`}
            disabled={pending}
            onClick={() => void setEnabled(!state.enabled)}
          >
            {pending ? 'Working...' : state.enabled ? 'Disable passkey' : 'Enable passkey'}
          </button>
        )}
      </div>
    </div>
  );
};
