import { Alert, AlertDescription, AlertTitle } from '@/components/alert';
import { Button } from '@/components/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/card';
import { Input } from '@/components/input';
import { Label } from '@/components/label';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import { CoreRequestError } from '@/utils/request';
import {
  ArrowRight,
  Check,
  Database,
  FileKey2,
  KeyRound,
  LoaderCircle,
  LockKeyhole,
  ShieldCheck,
  Upload,
} from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { StoredDatabase } from '@/types/Host';
import type { SubmitEvent } from 'react';

type FlowState = 'selecting' | 'importing' | 'locked' | 'unlocking' | 'unlocked';
const MAX_DATABASE_SIZE = 512 * 1024 * 1024;

const fileSize = (bytes: number) => {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
};

const errorMessage = (error: unknown) => {
  if (error instanceof CoreRequestError) {
    switch (error.code) {
      case 'invalid_credentials':
        return 'That password could not unlock this database.';
      case 'database_not_found':
        return 'The selected database is no longer available.';
      case 'storage_error':
        return 'The browser could not read the selected database.';
      default:
        return error.message;
    }
  }
  return error instanceof Error ? error.message : 'An unexpected error occurred.';
};

export const OpenFragment = () => {
  const { data: requestClient } = useRequestClient();
  const fileInputRef = useRef<HTMLInputElement>(null);
  const passwordRef = useRef<HTMLInputElement>(null);
  const [flow, setFlow] = useState<FlowState>('selecting');
  const [database, setDatabase] = useState<StoredDatabase | null>(null);
  const [storedDatabases, setStoredDatabases] = useState<StoredDatabase[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!requestClient) {
      return undefined;
    }
    const storage = requestClient.host.storages[0];
    if (!storage) {
      return undefined;
    }
    let active = true;
    void storage.listDatabases().then(
      databases => {
        if (active) {
          setStoredDatabases(databases);
        }
      },
      nextError => {
        if (active) {
          setError(errorMessage(nextError));
        }
      },
    );
    return () => {
      active = false;
    };
  }, [requestClient]);

  if (!requestClient) {
    return null;
  }

  const storage = requestClient.host.storages[0];

  const chooseFile = () => fileInputRef.current?.click();

  const openDatabase = async (selected: StoredDatabase) => {
    setFlow('importing');
    setError(null);
    try {
      await requestClient.request('open', { storage: selected.descriptor });
      setDatabase(selected);
      setFlow('locked');
      requestAnimationFrame(() => passwordRef.current?.focus());
    } catch (nextError) {
      setDatabase(null);
      setFlow('selecting');
      setError(errorMessage(nextError));
    }
  };

  const importFile = async (file: File) => {
    if (!storage) {
      setError('No browser storage provider is available.');
      return;
    }
    if (!file.name.toLowerCase().endsWith('.kdbx')) {
      setError('Choose a file with the .kdbx extension.');
      return;
    }
    if (file.size === 0 || file.size > MAX_DATABASE_SIZE) {
      setError('Choose a non-empty database smaller than 512 MB.');
      return;
    }
    setFlow('importing');
    setError(null);
    try {
      const selected = await storage.importDatabase(file);
      setStoredDatabases(databases => [...databases, selected]);
      await openDatabase(selected);
    } catch (nextError) {
      setDatabase(null);
      setFlow('selecting');
      setError(errorMessage(nextError));
    } finally {
      if (fileInputRef.current) {
        fileInputRef.current.value = '';
      }
    }
  };

  const onFileChange = (files: FileList | null) => {
    const file = files?.item(0);
    if (file) {
      void importFile(file);
    }
  };

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
      setFlow('locked');
      setError(errorMessage(nextError));
      requestAnimationFrame(() => input?.focus());
    }
  };

  const reset = async () => {
    try {
      await requestClient.request('lock', {});
      setDatabase(null);
      setError(null);
      setFlow('selecting');
    } catch (nextError) {
      setError(errorMessage(nextError));
    }
  };

  return (
    <main className="relative isolate flex min-h-dvh overflow-hidden bg-[#090b10] text-zinc-100">
      <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_16%_18%,rgba(70,92,190,0.2),transparent_32%),radial-gradient(circle_at_86%_76%,rgba(33,123,116,0.14),transparent_28%)]" />
      <div className="pointer-events-none absolute inset-0 opacity-30 [background-image:linear-gradient(rgba(255,255,255,.025)_1px,transparent_1px),linear-gradient(90deg,rgba(255,255,255,.025)_1px,transparent_1px)] [background-size:48px_48px]" />

      <section className="relative mx-auto grid w-full max-w-6xl items-center gap-12 px-6 py-10 lg:grid-cols-[1fr_27rem] lg:px-10">
        <div className="max-w-xl self-end pb-2 lg:self-center">
          <div className="mb-8 flex items-center gap-3 text-sm font-medium tracking-[0.18em] text-zinc-400 uppercase">
            <span className="grid size-9 place-items-center rounded-lg border border-white/10 bg-white/5">
              <KeyRound className="size-4 text-indigo-300" />
            </span>
            Keeless
          </div>
          <p className="mb-4 text-xs font-semibold tracking-[0.22em] text-indigo-300 uppercase">
            Local-first password vault
          </p>
          <h1 className="max-w-lg text-4xl leading-[1.05] font-semibold tracking-[-0.045em] text-balance sm:text-6xl">
            Your secrets stay where you put them.
          </h1>
          <p className="mt-6 max-w-md text-base leading-7 text-zinc-400">
            Open a KeePass database directly in this browser. Your encrypted file is stored locally
            and processed by the Keeless core.
          </p>
          <div className="mt-10 flex flex-wrap gap-x-6 gap-y-3 text-sm text-zinc-500">
            <span className="flex items-center gap-2">
              <ShieldCheck className="size-4 text-emerald-400" /> Authenticated core
            </span>
            <span className="flex items-center gap-2">
              <Database className="size-4 text-indigo-300" /> IndexedDB storage
            </span>
          </div>
        </div>

        <Card className="border-white/10 bg-zinc-950/75 text-zinc-100 shadow-2xl shadow-black/40 backdrop-blur-xl">
          <CardHeader className="border-b border-white/8 pb-5">
            <div className="mb-2 flex items-center justify-between">
              <span className="grid size-10 place-items-center rounded-xl bg-indigo-500/12 text-indigo-300">
                {flow === 'unlocked' ? (
                  <Check className="size-5" />
                ) : (
                  <LockKeyhole className="size-5" />
                )}
              </span>
              <span className="rounded-full border border-white/10 bg-white/5 px-2.5 py-1 text-[11px] font-medium tracking-wide text-zinc-400">
                BROWSER HOST
              </span>
            </div>
            <CardTitle className="text-xl">
              {flow === 'unlocked' ? 'Database unlocked' : 'Open your database'}
            </CardTitle>
            <CardDescription className="text-zinc-400">
              {flow === 'unlocked'
                ? 'The core is ready for encrypted requests.'
                : 'Choose a .kdbx file, then enter its master password.'}
            </CardDescription>
          </CardHeader>

          <CardContent className="pt-6">
            <Input
              ref={fileInputRef}
              type="file"
              accept=".kdbx,application/octet-stream"
              className="hidden"
              aria-label="Choose KeePass database"
              onChange={event => onFileChange(event.currentTarget.files)}
            />

            {!database && (
              <div className="space-y-4">
                {storedDatabases.length > 0 && (
                  <div>
                    <p className="mb-2 text-xs font-medium tracking-wide text-zinc-500 uppercase">
                      Stored in this browser
                    </p>
                    <div className="space-y-2">
                      {storedDatabases.map(stored => (
                        <button
                          key={stored.descriptor.path}
                          type="button"
                          disabled={flow === 'importing'}
                          onClick={() => void openDatabase(stored)}
                          className="flex w-full items-center gap-3 rounded-xl border border-white/10 bg-white/[0.035] p-3 text-start transition-colors hover:border-indigo-400/35 hover:bg-indigo-400/[0.04] focus-visible:ring-2 focus-visible:ring-indigo-400/60 focus-visible:outline-none disabled:pointer-events-none disabled:opacity-60"
                        >
                          <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-indigo-500/10 text-indigo-300">
                            <FileKey2 className="size-4" />
                          </span>
                          <span className="min-w-0 flex-1">
                            <span className="block truncate text-sm font-medium">
                              {stored.name}
                            </span>
                            <span className="text-xs text-zinc-500">{fileSize(stored.size)}</span>
                          </span>
                          <ArrowRight className="size-4 text-zinc-600" />
                        </button>
                      ))}
                    </div>
                  </div>
                )}

                <button
                  type="button"
                  disabled={flow === 'importing'}
                  onClick={chooseFile}
                  className="group flex min-h-40 w-full flex-col items-center justify-center rounded-xl border border-dashed border-white/15 bg-white/[0.025] px-6 text-center transition-colors hover:border-indigo-400/50 hover:bg-indigo-400/[0.04] focus-visible:ring-2 focus-visible:ring-indigo-400/60 focus-visible:outline-none disabled:pointer-events-none disabled:opacity-60"
                >
                  <span className="mb-4 grid size-11 place-items-center rounded-full border border-white/10 bg-white/5 text-zinc-300 transition-transform group-hover:-translate-y-0.5">
                    {flow === 'importing' ? (
                      <LoaderCircle className="size-5 animate-spin" />
                    ) : (
                      <Upload className="size-5" />
                    )}
                  </span>
                  <span className="font-medium">
                    {flow === 'importing' ? 'Opening database...' : 'Import another database'}
                  </span>
                  <span className="mt-2 text-sm text-zinc-500">.kdbx files up to 512 MB</span>
                </button>
              </div>
            )}

            {database && flow !== 'unlocked' && (
              <form onSubmit={unlock}>
                <div className="mb-6 flex items-center gap-3 rounded-xl border border-white/10 bg-white/[0.035] p-3.5">
                  <span className="grid size-10 shrink-0 place-items-center rounded-lg bg-indigo-500/10 text-indigo-300">
                    <FileKey2 className="size-5" />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm font-medium">{database.name}</span>
                    <span className="text-xs text-zinc-500">{fileSize(database.size)}</span>
                  </span>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    disabled={flow === 'unlocking'}
                    onClick={() => void reset()}
                  >
                    Change
                  </Button>
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

            {database && flow === 'unlocked' && (
              <div className="py-6 text-center">
                <span className="mx-auto mb-5 grid size-16 place-items-center rounded-full border border-emerald-400/20 bg-emerald-400/10 text-emerald-300">
                  <ShieldCheck className="size-7" />
                </span>
                <p className="font-medium">{database.name}</p>
                <p className="mt-2 text-sm leading-6 text-zinc-500">
                  The database UI will appear here as core operations are added.
                </p>
                <Button
                  type="button"
                  variant="outline"
                  className="mt-6"
                  onClick={() => void reset()}
                >
                  Open another database
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
              <ShieldCheck className="size-3.5" /> The original file never leaves this device.
            </p>
          </CardContent>
        </Card>
      </section>
    </main>
  );
};
