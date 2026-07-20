import { Alert, AlertDescription, AlertTitle } from '@/components/alert';
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
import { IconAlertCircle, IconArrowRight, IconLoaderCircle } from '@/icons';
import { SetupLayout } from './SetupLayout';
import { StepError } from './StepError';
import type { Host, HostKind, HostStorage } from '@/types/Host';

export const SelectStep = ({
  host,
  hosts,
  storage,
  hostsLoading,
  isHostOverride,
  isPending,
  isRequestReady,
  requestError,
  error,
  onSelectHost,
  onChooseStorage,
  onRetryRequest,
}: {
  host?: Host;
  hosts: Host[];
  storage?: HostStorage;
  hostsLoading: boolean;
  isHostOverride: boolean;
  isPending: boolean;
  isRequestReady: boolean;
  requestError?: string;
  error?: string;
  onSelectHost: (kind: HostKind) => void;
  onChooseStorage: (storage: HostStorage) => void;
  onRetryRequest: () => void;
}) => (
  <SetupLayout
    title="Open a database"
    description="Choose where Keeless should run and store its database."
  >
    <div className="space-y-2">
      <Select
        value={host?.kind ?? null}
        onValueChange={value => value && onSelectHost(value)}
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
                  disabled={isPending || !isRequestReady}
                />
              }
              aria-pressed={storage?.kind === candidate.kind}
              onClick={() => onChooseStorage(candidate)}
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

    {host && requestError && (
      <Alert variant="destructive">
        <IconAlertCircle />
        <AlertTitle>Host could not start</AlertTitle>
        <AlertDescription>{requestError}</AlertDescription>
        <Button type="button" variant="outline" className="mt-2" onClick={onRetryRequest}>
          Try again
        </Button>
      </Alert>
    )}
    <StepError error={error} />
  </SetupLayout>
);
