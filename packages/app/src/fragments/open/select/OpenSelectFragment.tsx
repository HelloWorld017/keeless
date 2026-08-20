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
import { useNavigate } from '@/fragments/_providers/RouterProvider';
import { IconAlertCircle, IconArrowRight, IconLoaderCircle } from '@/icons';
import { buildRoute } from '@/utils/route';
import { useRef, useState } from 'react';
import { SetupLayout } from '../_components/SetupLayout';
import { StepError } from '../_components/StepError';
import { errorMessage } from '../_utils/errorMessage';
import type { OpenStepChange } from '../_types/Step';
import type { HostKind, HostStorage, StorageProviderGetter } from '@/types/Host';

export const OpenSelectFragment = ({
  onStepChange,
  onBack,
}: {
  onStepChange: OpenStepChange;
  onBack: () => void;
}) => {
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

  const openStorage = async (getProvider: StorageProviderGetter) => {
    if (!client || operationPendingRef.current) {
      return;
    }

    operationPendingRef.current = true;
    setIsPending(true);
    setError(undefined);
    try {
      await client.request('open', { storage: await getProvider() });
      const { database } = await client.request('getCoreStatus', {});
      if (database === 'unlocked') {
        await client.upgrade();
        navigate(buildRoute('database'), { replace: true });
      } else {
        onStepChange({ kind: database === 'locked' ? 'unlock' : 'create' });
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
      onStepChange({ kind: 'storage', storage: nextStorage.kind });
      return;
    }
    void openStorage(nextStorage.setup.getDefaultProvider);
  };

  const selectHostKind = (kind: HostKind) => {
    selectHost(kind);
    setStorage(undefined);
    setError(undefined);
  };

  return (
    <SetupLayout
      title="Open a database"
      description="Choose where Keeless should run and store its database."
      onBack={onBack}
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
