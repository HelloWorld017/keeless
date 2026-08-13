import BackgroundImage from '@/assets/images/background.webp?url';
import { Alert, AlertAction, AlertDescription, AlertTitle } from '@/components/alert';
import { Button } from '@/components/button';
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemMedia,
  ItemTitle,
} from '@/components/item';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from '@/components/select';
import {
  useHost,
  useHostOverride,
  useHosts,
  useHostsLoading,
  useSelectHost,
} from '@/fragments/_providers/HostProvider';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import { IconAlertCircle, IconArrowRight, IconLoaderCircle } from '@/icons';
import { buildRoute, getRoute } from '@/utils/route';
import { useEffect, useRef, useState } from 'react';
import { Redirect, Route, Switch } from 'wouter';
import { useNavigate } from '../_providers/RouterProvider';
import { SetupLayout } from './_components/SetupLayout';
import { StepError } from './_components/StepError';
import { errorMessage } from './_utils/errorMessage';
import { OpenCreateFragment } from './create/OpenCreateFragment';
import { OpenStorageFragment } from './storage/OpenStorageFragment';
import { OpenUnlockFragment } from './unlock/OpenUnlockFragment';
import type { HostKind, HostStorage, StorageDescriptorGetter } from '@/types/Host';

const SelectStep = () => {
  const host = useHost();
  const hosts = useHosts();
  const hostsLoading = useHostsLoading();
  const isHostOverride = useHostOverride();
  const selectHost = useSelectHost();
  const requestClient = useRequestClient();
  const client = requestClient.data;
  const navigate = useNavigate();
  const operationPendingRef = useRef(false);
  const [storage, setStorage] = useState<HostStorage>();
  const [isPending, setIsPending] = useState(false);
  const [error, setError] = useState<string>();
  const [isChecking, setIsChecking] = useState(false);
  const [checkingError, setCheckingError] = useState<string>();
  const [checkingAttempt, setCheckingAttempt] = useState(0);

  useEffect(() => {
    if (!isHostOverride || !client) {
      return undefined;
    }

    let active = true;
    setIsChecking(true);
    setCheckingError(undefined);
    void client
      .request('getCoreStatus', {})
      .then(async ({ database }) => {
        if (!active) {
          return;
        }
        if (database === 'unlocked') {
          await client.upgrade();
          if (active) {
            navigate(buildRoute('database'), { replace: true });
          }
          return;
        }
        if (database === 'locked') {
          navigate(buildRoute('openUnlock'), { replace: true });
        }
      })
      .catch(nextError => {
        if (active) {
          setCheckingError(errorMessage(nextError));
        }
      })
      .finally(() => {
        if (active) {
          setIsChecking(false);
        }
      });
    return () => {
      active = false;
    };
  }, [checkingAttempt, client, isHostOverride, navigate]);

  const openStorage = async (getDescriptor: StorageDescriptorGetter) => {
    if (!client || operationPendingRef.current) {
      return;
    }

    operationPendingRef.current = true;
    setIsPending(true);
    setError(undefined);
    try {
      const descriptor = await getDescriptor();
      await client.request('open', { storage: descriptor });
      const { database } = await client.request('getCoreStatus', {});
      if (database === 'unlocked') {
        await client.upgrade();
        navigate(buildRoute('database'), { replace: true });
      } else {
        navigate(buildRoute(database === 'locked' ? 'openUnlock' : 'openCreate'));
      }
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
    if (nextStorage.setup.component) {
      navigate(buildRoute('openStorage', { storage: nextStorage.kind }));
      return;
    }
    void openStorage(nextStorage.setup.getDefaultDescriptor);
  };

  if (
    isHostOverride &&
    (requestClient.isPending || requestClient.isError || isChecking || checkingError)
  ) {
    const checkingPending = requestClient.isFetching || isChecking;
    const checkingFailure =
      checkingError ?? (requestClient.isError ? errorMessage(requestClient.error) : undefined);

    return (
      <SetupLayout
        title="Opening database"
        description="Checking the selected host."
        showBack={false}
      >
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          {checkingPending && <IconLoaderCircle className="animate-spin" />}
          {checkingPending
            ? 'Checking database status...'
            : 'Database status could not be checked.'}
        </div>
        <StepError error={checkingFailure} />
        {checkingFailure && (
          <Button
            type="button"
            variant="outline"
            onClick={() => {
              setCheckingError(undefined);
              setIsChecking(true);
              if (requestClient.isError) {
                void requestClient.refetch();
                return;
              }
              setCheckingAttempt(current => current + 1);
            }}
          >
            Try again
          </Button>
        )}
      </SetupLayout>
    );
  }

  const selectHostKind = (kind: HostKind) => {
    selectHost(kind);
    setStorage(undefined);
    setError(undefined);
  };

  return (
    <SetupLayout
      title="Open a database"
      description="Choose where Keeless should run and store its database."
      showBack={false}
    >
      <div className="space-y-2">
        <Select
          value={host?.kind ?? null}
          onValueChange={value => value && selectHostKind(value)}
          disabled={isHostOverride || hostsLoading || isPending}
        >
          <SelectTrigger id="host" className="w-full max-w-48">
            <SelectValue
              placeholder={hostsLoading ? 'Looking for hosts...' : 'Select a host'}
              className="capitalize"
            />
          </SelectTrigger>
          <SelectContent>
            <SelectGroup>
              <SelectLabel>Hosts</SelectLabel>
              {hosts.map(candidate => (
                <SelectItem key={candidate.kind} value={candidate.kind} className="capitalize">
                  {candidate.label}
                </SelectItem>
              ))}
            </SelectGroup>
          </SelectContent>
        </Select>
      </div>

      {host && (
        <div className="space-y-2">
          <ItemGroup className="gap-2">
            {host.storages.map(candidate => (
              <Item
                key={candidate.kind}
                variant="outline"
                render={
                  <button
                    type="button"
                    className="transition-colors hover:bg-muted"
                    aria-label={`Use ${candidate.label}`}
                    disabled={isPending || !client}
                  />
                }
                aria-pressed={storage?.kind === candidate.kind}
                onClick={() => chooseStorage(candidate)}
              >
                <ItemMedia variant="icon">{candidate.icon}</ItemMedia>
                <ItemContent className="gap-0">
                  <ItemTitle className="font-semibold">{candidate.label}</ItemTitle>
                  <ItemDescription>{candidate.description}</ItemDescription>
                </ItemContent>
                <ItemActions>
                  {isPending && storage?.kind === candidate.kind ? (
                    <IconLoaderCircle className="animate-spin" />
                  ) : (
                    <IconArrowRight />
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
          <AlertAction>
            <Button
              type="button"
              variant="outline"
              className="mt-2"
              onClick={() => void requestClient.refetch()}
            >
              Try again
            </Button>
          </AlertAction>
        </Alert>
      )}
      <StepError error={error} />
    </SetupLayout>
  );
};

export const OpenFragment = () => (
  <div className="flex h-dvh items-center">
    <div className="flex-[0_0_auto] max-w-200 w-full">
      <Switch>
        <Route path={getRoute('openStorage')} component={OpenStorageFragment} />
        <Route path={getRoute('openCreate')} component={OpenCreateFragment} />
        <Route path={getRoute('openUnlock')} component={OpenUnlockFragment} />
        <Route path={getRoute('open')} component={SelectStep} />
        <Redirect to={getRoute('open')} replace />
      </Switch>
    </div>
    <div className="p-6 flex-[1_1_0] self-stretch">
      <img
        src={BackgroundImage}
        alt=""
        className="w-full h-full grayscale object-cover rounded-[30px]"
      />
    </div>
  </div>
);
