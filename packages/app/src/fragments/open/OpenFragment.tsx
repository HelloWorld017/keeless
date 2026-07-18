import { Alert, AlertDescription, AlertTitle } from '@/components/alert';
import { Button } from '@/components/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/card';
import { Input } from '@/components/input';
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemTitle,
} from '@/components/item';
import { Label } from '@/components/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/select';
import {
  useHost,
  useHostOverride,
  useHosts,
  useHostsLoading,
  usePasswordInput,
  useSelectHost,
} from '@/fragments/_providers/HostProvider';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import { IconAlertCircle, IconChevronLeft, IconChevronRight, IconLoaderCircle } from '@/icons';
import { CoreRequestError } from '@/utils/request';
import { buildRoute } from '@/utils/route';
import { useEffect, useRef, useState } from 'react';
import { useNavigate } from '../_providers/RouterProvider';
import type { PasswordInputMode } from '@/types/AppIntegration';
import type { HostStorage, HostStorageInput } from '@/types/Host';
import type { DatabaseStatus } from '@keeless/schema';
import type { SubmitEvent } from 'react';

type SetupStep = 'select' | 'details' | 'create' | 'unlock' | 'checking';

const errorMessage = (error: unknown) => {
  if (error instanceof CoreRequestError) {
    switch (error.code) {
      case 'invalid_credentials':
        return 'That password could not unlock this database.';
      case 'database_not_found':
        return 'The database no longer exists.';
      case 'storage_error':
        return 'The storage could not be accessed.';
      default:
        return error.message;
    }
  }
  return error instanceof Error ? error.message : 'An unexpected error occurred.';
};

const formString = (data: FormData, name: string) => {
  const value = data.get(name);
  return typeof value === 'string' ? value : '';
};

export const OpenFragment = () => {
  const host = useHost();
  const hosts = useHosts();
  const hostsLoading = useHostsLoading();
  const isHostOverride = useHostOverride();
  const onPasswordInput = usePasswordInput();
  const selectHost = useSelectHost();
  const requestClient = useRequestClient();
  const navigate = useNavigate();
  const passwordRef = useRef<HTMLInputElement>(null);
  const operationPendingRef = useRef(false);
  const [step, setStep] = useState<SetupStep>(isHostOverride ? 'checking' : 'select');
  const [storage, setStorage] = useState<HostStorage>();
  const [isPending, setIsPending] = useState(false);
  const [error, setError] = useState<string>();

  useEffect(() => {
    if (!isHostOverride || !requestClient.data) {
      return undefined;
    }

    let active = true;
    setStep('checking');
    setIsPending(true);
    setError(undefined);
    void requestClient.data
      .request('getDatabaseStatus', {})
      .then(({ status }) => {
        if (!active) {
          return;
        }
        if (status === 'unlocked') {
          navigate(buildRoute('database'), { replace: true });
        } else {
          setStep(status === 'locked' ? 'unlock' : 'select');
        }
      })
      .catch(nextError => {
        if (active) {
          setError(errorMessage(nextError));
        }
      })
      .finally(() => {
        if (active) {
          setIsPending(false);
        }
      });
    return () => {
      active = false;
    };
  }, [isHostOverride, navigate, requestClient.data]);

  const moveFromStatus = (status: DatabaseStatus) => {
    if (status === 'unlocked') {
      navigate(buildRoute('database'), { replace: true });
      return;
    }
    setStep(status === 'locked' ? 'unlock' : 'create');
  };

  const openStorage = async (input: HostStorageInput) => {
    if (!host || !requestClient.data || operationPendingRef.current) {
      return;
    }
    operationPendingRef.current = true;
    setIsPending(true);
    setError(undefined);
    try {
      let descriptor;
      try {
        descriptor = await host.configureStorage(input);
      } finally {
        if (input.kind === 'webdav') {
          input.password = '';
        }
      }
      await requestClient.data.request('open', { storage: descriptor });
      const { status } = await requestClient.data.request('getDatabaseStatus', {});
      moveFromStatus(status);
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      operationPendingRef.current = false;
      setIsPending(false);
    }
  };

  const chooseStorage = (nextStorage: HostStorage) => {
    setStorage(nextStorage);
    setError(undefined);
    if (nextStorage.requiresDetails) {
      setStep('details');
      return;
    }
    void openStorage({ kind: 'indexeddb' });
  };

  const submitWebDav = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const passwordInput = form.elements.namedItem('password');
    const input: HostStorageInput = {
      kind: 'webdav',
      url: formString(data, 'url').trim(),
      username: formString(data, 'username').trim(),
      password: formString(data, 'password'),
      path: formString(data, 'path').trim(),
    };
    if (passwordInput instanceof HTMLInputElement) {
      passwordInput.value = '';
    }
    void openStorage(input);
  };

  const requestPassword = async (
    mode: PasswordInputMode,
    form?: HTMLFormElement,
  ): Promise<string | null> => {
    if (onPasswordInput) {
      return onPasswordInput(mode);
    }
    const input = form?.elements.namedItem('master-password');
    if (!(input instanceof HTMLInputElement) || !input.value) {
      setError('Enter the master password for this database.');
      passwordRef.current?.focus();
      return null;
    }
    const password = input.value;
    input.value = '';
    return password;
  };

  const submitPassword = async (mode: PasswordInputMode, event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!requestClient.data || operationPendingRef.current) {
      return;
    }
    operationPendingRef.current = true;
    setIsPending(true);
    setError(undefined);
    try {
      const password = await requestPassword(mode, event.currentTarget);
      if (!password) {
        return;
      }
      await requestClient.data.request(mode, { password });
      navigate(buildRoute('database'), { replace: true });
    } catch (nextError) {
      if (
        mode === 'create' &&
        nextError instanceof CoreRequestError &&
        nextError.code === 'database_already_exists'
      ) {
        try {
          const { status } = await requestClient.data.request('getDatabaseStatus', {});
          moveFromStatus(status);
        } catch (statusError) {
          setError(errorMessage(statusError));
        }
        return;
      }
      if (
        mode === 'unlock' &&
        nextError instanceof CoreRequestError &&
        nextError.code === 'database_not_found'
      ) {
        try {
          const { status } = await requestClient.data.request('getDatabaseStatus', {});
          moveFromStatus(status);
        } catch (statusError) {
          setError(errorMessage(statusError));
        }
        return;
      }
      setError(errorMessage(nextError));
      requestAnimationFrame(() => passwordRef.current?.focus());
    } finally {
      operationPendingRef.current = false;
      setIsPending(false);
    }
  };

  const renderError = () =>
    error && (
      <Alert variant="destructive">
        <IconAlertCircle />
        <AlertTitle>Could not continue</AlertTitle>
        <AlertDescription>{error}</AlertDescription>
      </Alert>
    );

  if (step === 'checking') {
    const checkingPending = requestClient.isPending || isPending;
    const checkingError =
      error ?? (requestClient.isError ? errorMessage(requestClient.error) : undefined);
    return (
      <SetupLayout title="Opening database" description="Checking the selected host.">
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          {checkingPending && <IconLoaderCircle className="animate-spin" />}
          {checkingPending
            ? 'Checking database status...'
            : 'Database status could not be checked.'}
        </div>
        {checkingError && (
          <Alert variant="destructive">
            <IconAlertCircle />
            <AlertTitle>Could not continue</AlertTitle>
            <AlertDescription>{checkingError}</AlertDescription>
          </Alert>
        )}
        {checkingError && (
          <Button type="button" variant="outline" onClick={() => void requestClient.refetch()}>
            Try again
          </Button>
        )}
      </SetupLayout>
    );
  }

  if (step === 'details') {
    return (
      <SetupLayout
        title="Connect WebDAV"
        description="Enter the connection details for your server."
      >
        <form className="space-y-4" onSubmit={submitWebDav}>
          <FormInput label="URL" name="url" type="url" autoComplete="url" required />
          <FormInput label="User" name="username" autoComplete="username" required />
          <FormInput
            label="Password"
            name="password"
            type="password"
            autoComplete="current-password"
            required
          />
          <FormInput label="Path (optional)" name="path" placeholder="keeless.kdbx" />
          {renderError()}
          <div className="flex justify-between gap-3 pt-2">
            <Button
              type="button"
              variant="outline"
              disabled={isPending}
              onClick={() => {
                setError(undefined);
                setStep('select');
              }}
            >
              <IconChevronLeft /> Back
            </Button>
            <Button type="submit" disabled={isPending}>
              {isPending && <IconLoaderCircle className="animate-spin" />}
              Continue
            </Button>
          </div>
        </form>
      </SetupLayout>
    );
  }

  if (step === 'create' || step === 'unlock') {
    const isCreate = step === 'create';
    return (
      <SetupLayout
        title={isCreate ? 'Create database' : 'Unlock database'}
        description={
          isCreate
            ? 'Choose the master password for this database.'
            : 'Enter the master password for this database.'
        }
      >
        <form onSubmit={event => void submitPassword(step, event)} className="space-y-4">
          {!onPasswordInput && (
            <div className="space-y-2">
              <Label htmlFor="master-password">Master password</Label>
              <Input
                ref={passwordRef}
                id="master-password"
                name="master-password"
                type="password"
                autoComplete={isCreate ? 'new-password' : 'current-password'}
                disabled={isPending}
                required
              />
            </div>
          )}
          {onPasswordInput && (
            <p className="text-sm text-muted-foreground">
              The master password will be entered in a secure system prompt.
            </p>
          )}
          {renderError()}
          <Button type="submit" className="w-full" disabled={isPending}>
            {isPending && <IconLoaderCircle className="animate-spin" />}
            {onPasswordInput
              ? 'Enter master password'
              : isCreate
                ? 'Create database'
                : 'Unlock database'}
          </Button>
        </form>
      </SetupLayout>
    );
  }

  return (
    <SetupLayout
      title="Open a database"
      description="Choose where Keeless should run and store its database."
    >
      <div className="space-y-2">
        <Label htmlFor="host">Host</Label>
        <Select
          value={host?.kind ?? null}
          onValueChange={value => {
            if (value) {
              selectHost(value);
              setStorage(undefined);
              setError(undefined);
            }
          }}
          disabled={isHostOverride || hostsLoading || isPending}
        >
          <SelectTrigger id="host" className="w-full">
            <SelectValue placeholder={hostsLoading ? 'Looking for hosts...' : 'Select a host'} />
          </SelectTrigger>
          <SelectContent>
            {hosts.map(candidate => (
              <SelectItem key={candidate.kind} value={candidate.kind}>
                {candidate.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {host && (
        <div className="space-y-2">
          <Label>Storage</Label>
          <ItemGroup className="gap-2">
            {host.storages.map(candidate => (
              <Item
                key={candidate.kind}
                variant="outline"
                render={
                  <button
                    type="button"
                    aria-label={`Use ${candidate.label}`}
                    disabled={isPending || !requestClient.data}
                  />
                }
                aria-pressed={storage?.kind === candidate.kind}
                onClick={() => chooseStorage(candidate)}
              >
                <ItemContent>
                  <ItemTitle>{candidate.label}</ItemTitle>
                  <ItemDescription>{candidate.description}</ItemDescription>
                </ItemContent>
                <ItemActions>
                  {isPending && storage?.kind === candidate.kind ? (
                    <IconLoaderCircle className="animate-spin" />
                  ) : (
                    <IconChevronRight />
                  )}
                </ItemActions>
              </Item>
            ))}
          </ItemGroup>
        </div>
      )}

      {!hostsLoading && hosts.length === 0 && (
        <Alert>
          <AlertTitle>No compatible host</AlertTitle>
          <AlertDescription>
            Keeless could not find an available host on this device.
          </AlertDescription>
        </Alert>
      )}

      {host && requestClient.isError && (
        <Alert variant="destructive">
          <IconAlertCircle />
          <AlertTitle>Host could not start</AlertTitle>
          <AlertDescription>{errorMessage(requestClient.error)}</AlertDescription>
          <Button
            type="button"
            variant="outline"
            className="mt-2"
            onClick={() => void requestClient.refetch()}
          >
            Try again
          </Button>
        </Alert>
      )}
      {renderError()}
    </SetupLayout>
  );
};

const SetupLayout = ({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: React.ReactNode;
}) => (
  <main className="flex min-h-dvh items-center justify-center px-4 py-10">
    <Card className="w-full max-w-md">
      <CardHeader>
        <CardTitle>{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-5">{children}</CardContent>
    </Card>
  </main>
);

const FormInput = ({ label, ...props }: { label: string } & React.ComponentProps<'input'>) => (
  <div className="space-y-2">
    <Label htmlFor={props.name}>{label}</Label>
    <Input id={props.name} {...props} />
  </div>
);
