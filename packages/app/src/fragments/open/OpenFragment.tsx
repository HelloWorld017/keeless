import { Alert, AlertDescription, AlertTitle } from '@/components/alert';
import { Button } from '@/components/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/card';
import { Input } from '@/components/input';
import { Label } from '@/components/label';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import { CoreRequestError } from '@/utils/request';
import { ArrowRight, Check, FileKey2, LoaderCircle, LockKeyhole, ShieldCheck } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { DatabaseStatus } from '@keeless/schema';
import type { SubmitEvent } from 'react';

type FlowState = 'loading' | 'unlocking' | DatabaseStatus;

const DATABASE_NAME = 'keeless.kdbx';

const errorMessage = (error: unknown) => {
  if (error instanceof CoreRequestError) {
    switch (error.code) {
      case 'invalid_credentials':
        return 'That password could not unlock this database.';
      case 'database_not_found':
        return `${DATABASE_NAME} no longer exists in this browser.`;
      case 'storage_error':
        return `The browser could not read ${DATABASE_NAME}.`;
      default:
        return error.message;
    }
  }
  return error instanceof Error ? error.message : 'An unexpected error occurred.';
};

export const OpenFragment = () => {
  const { data: requestClient } = useRequestClient();
  const passwordRef = useRef<HTMLInputElement>(null);
  const [flow, setFlow] = useState<FlowState>('loading');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!requestClient) {
      return undefined;
    }
    let active = true;
    const open = async () => {
      setFlow('loading');
      setError(null);
      try {
        await requestClient.request('open', { storage: requestClient.host.database });
        const { status } = await requestClient.request('getDatabaseStatus', {});
        if (!active) {
          return;
        }
        setFlow(status);
        if (status === 'locked') {
          requestAnimationFrame(() => passwordRef.current?.focus());
        }
      } catch (nextError) {
        if (active) {
          setFlow('not_exist');
          setError(errorMessage(nextError));
        }
      }
    };
    void open();
    return () => {
      active = false;
    };
  }, [requestClient]);

  if (!requestClient) {
    return null;
  }

  const unlock = async (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    const input = passwordRef.current;
    const password = input?.value ?? '';
    if (!password) {
      setError('Enter the master password for this database.');
      input?.focus();
      return;
    }
    setFlow('unlocking');
    setError(null);
    if (input) {
      input.value = '';
    }
    try {
      await requestClient.request('unlock', { password });
      setFlow('unlocked');
    } catch (nextError) {
      setFlow(
        nextError instanceof CoreRequestError && nextError.code === 'database_not_found'
          ? 'not_exist'
          : 'locked',
      );
      setError(errorMessage(nextError));
      requestAnimationFrame(() => input?.focus());
    }
  };

  const lock = async () => {
    setError(null);
    try {
      await requestClient.request('lock', {});
      setFlow('locked');
      requestAnimationFrame(() => passwordRef.current?.focus());
    } catch (nextError) {
      setError(errorMessage(nextError));
    }
  };

  return (
    <main className="relative isolate flex min-h-dvh overflow-hidden bg-[#090b10] text-zinc-100">
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_16%_18%,rgba(70,92,190,0.2),transparent_32%),radial-gradient(circle_at_86%_76%,rgba(33,123,116,0.14),transparent_28%)]" />
      <div className="pointer-events-none absolute inset-0 opacity-30 [background-image:linear-gradient(rgba(255,255,255,.025)_1px,transparent_1px),linear-gradient(90deg,rgba(255,255,255,.025)_1px,transparent_1px)] [background-size:48px_48px]" />

      <section className="relative mx-auto grid w-full max-w-6xl items-center gap-12 px-6 py-10 lg:grid-cols-[1fr_27rem] lg:px-10">
        <Card className="border-white/10 bg-zinc-950/75 text-zinc-100 shadow-2xl shadow-black/40 backdrop-blur-xl">
          <CardHeader className="border-b border-white/8 pb-5">
            <div className="mb-2 flex items-center justify-between">
              <span className="grid size-10 place-items-center rounded-xl bg-indigo-500/12 text-indigo-300">
                {flow === 'unlocked' ? (
                  <Check className="size-5" />
                ) : flow === 'loading' ? (
                  <LoaderCircle className="size-5 animate-spin" />
                ) : (
                  <LockKeyhole className="size-5" />
                )}
              </span>
              <span className="rounded-full border border-white/10 bg-white/5 px-2.5 py-1 text-[11px] font-medium tracking-wide text-zinc-400">
                BROWSER HOST
              </span>
            </div>
            <CardTitle className="text-xl">
              {flow === 'unlocked'
                ? 'Database unlocked'
                : flow === 'not_exist'
                  ? 'Database not found'
                  : flow === 'loading'
                    ? 'Opening database'
                    : 'Unlock your database'}
            </CardTitle>
            <CardDescription className="text-zinc-400">
              {flow === 'unlocked'
                ? 'The core is ready for encrypted requests.'
                : flow === 'not_exist'
                  ? `${DATABASE_NAME} has not been created in this browser.`
                  : flow === 'loading'
                    ? `Looking for ${DATABASE_NAME} in IndexedDB.`
                    : `Enter the master password for ${DATABASE_NAME}.`}
            </CardDescription>
          </CardHeader>

          <CardContent className="pt-6">
            {flow === 'loading' && (
              <div className="py-12 text-center text-zinc-400">
                <LoaderCircle className="mx-auto mb-4 size-7 animate-spin text-indigo-300" />
                <p className="text-sm">Opening {DATABASE_NAME}...</p>
              </div>
            )}

            {flow === 'not_exist' && (
              <div className="py-8 text-center">
                <span className="mx-auto mb-5 grid size-16 place-items-center rounded-full border border-white/10 bg-white/5 text-zinc-400">
                  <FileKey2 className="size-7" />
                </span>
                <p className="font-medium">No local database</p>
                <p className="mx-auto mt-2 max-w-sm text-sm leading-6 text-zinc-500">
                  Database creation is not available yet. This browser will always use{' '}
                  {DATABASE_NAME} when creation support is added.
                </p>
              </div>
            )}

            {(flow === 'locked' || flow === 'unlocking') && (
              <form onSubmit={unlock}>
                <div className="mb-6 flex items-center gap-3 rounded-xl border border-white/10 bg-white/[0.035] p-3.5">
                  <span className="grid size-10 shrink-0 place-items-center rounded-lg bg-indigo-500/10 text-indigo-300">
                    <FileKey2 className="size-5" />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm font-medium">{DATABASE_NAME}</span>
                    <span className="text-xs text-zinc-500">IndexedDB</span>
                  </span>
                </div>

                <div className="space-y-2">
                  <Label htmlFor="master-password" className="text-zinc-300">
                    Master password
                  </Label>
                  <Input
                    ref={passwordRef}
                    id="master-password"
                    type="password"
                    autoComplete="current-password"
                    disabled={flow === 'unlocking'}
                    className="h-10 border-white/10 bg-white/5 text-zinc-100 placeholder:text-zinc-600"
                    placeholder="Enter your password"
                  />
                </div>

                <Button
                  type="submit"
                  size="lg"
                  className="mt-5 h-10 w-full"
                  disabled={flow === 'unlocking'}
                >
                  {flow === 'unlocking' ? (
                    <>
                      <LoaderCircle className="animate-spin" /> Unlocking
                    </>
                  ) : (
                    <>
                      Unlock database <ArrowRight />
                    </>
                  )}
                </Button>
              </form>
            )}

            {flow === 'unlocked' && (
              <div className="py-6 text-center">
                <span className="mx-auto mb-5 grid size-16 place-items-center rounded-full border border-emerald-400/20 bg-emerald-400/10 text-emerald-300">
                  <ShieldCheck className="size-7" />
                </span>
                <p className="font-medium">{DATABASE_NAME}</p>
                <p className="mt-2 text-sm leading-6 text-zinc-500">
                  The database UI will appear here as core operations are added.
                </p>
                <Button
                  type="button"
                  variant="outline"
                  className="mt-6"
                  onClick={() => void lock()}
                >
                  Lock database
                </Button>
              </div>
            )}

            {error && (
              <Alert
                variant="destructive"
                className="mt-5 border-red-400/20 bg-red-400/8 text-red-200"
              >
                <AlertTitle>Could not continue</AlertTitle>
                <AlertDescription className="text-red-200/75">{error}</AlertDescription>
              </Alert>
            )}

            <p className="mt-6 flex items-center justify-center gap-2 text-center text-xs text-zinc-600">
              <ShieldCheck className="size-3.5" /> The database never leaves this device.
            </p>
          </CardContent>
        </Card>
      </section>
    </main>
  );
};
