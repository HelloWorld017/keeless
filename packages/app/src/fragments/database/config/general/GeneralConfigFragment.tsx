import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/select';
import { Switch } from '@/components/switch';
import { useRequest, useRequestClient } from '@/fragments/_providers/QueryProvider';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { ConfigRow } from '../_components';
import type { KeelessConfigPatch } from '@keeless/schema';

const autoLockOptions: ReadonlyArray<{ value: string; label: string; timeout: number | null }> = [
  { value: 'off', label: 'Off', timeout: null },
  { value: '1m', label: '1 minute', timeout: 60_000 },
  { value: '5m', label: '5 minutes', timeout: 5 * 60_000 },
  { value: '15m', label: '15 minutes', timeout: 15 * 60_000 },
  { value: '30m', label: '30 minutes', timeout: 30 * 60_000 },
  { value: '1h', label: '1 hour', timeout: 60 * 60_000 },
] as const;

const getAutoLockOptions = (timeout: number | null | undefined) => {
  if (timeout === undefined || autoLockOptions.some(option => option.timeout === timeout)) {
    return autoLockOptions;
  }

  return [{ value: `custom-${timeout}`, label: `${timeout} ms`, timeout }, ...autoLockOptions];
};

export const GeneralConfigFragment = () => {
  const queryClient = useQueryClient();
  const requestClient = useRequestClient();
  const config = useRequest('getConfig', {});
  const setConfig = useMutation({
    mutationFn: (patch: KeelessConfigPatch) =>
      requestClient.data!.request('setConfig', { config: patch }),
    onSuccess: async (_result, patch) => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['request', 'getConfig', {}] }),
        ...(patch.paranoiaMode === undefined
          ? []
          : [queryClient.invalidateQueries({ queryKey: ['request', 'getDatabaseStatus', {}] })]),
      ]);
    },
  });

  if (config.isError) {
    return (
      <p className="text-sm text-destructive" role="alert">
        Configuration could not be loaded.
      </p>
    );
  }

  const currentConfig = config.data?.config;
  const availableAutoLockOptions = getAutoLockOptions(currentConfig?.autoLockTimeoutMs);
  const autoLockOption = availableAutoLockOptions.find(
    option => option.timeout === currentConfig?.autoLockTimeoutMs,
  );
  const disabled = !currentConfig || setConfig.isPending;

  return (
    <div>
      <ConfigRow title="Auto lock" description="Lock the database after a period of inactivity.">
        <Select
          value={autoLockOption?.value ?? null}
          disabled={disabled}
          onValueChange={value => {
            const option = availableAutoLockOptions.find(candidate => candidate.value === value);
            if (option) {
              setConfig.mutate({ autoLockTimeoutMs: option.timeout });
            }
          }}
        >
          <SelectTrigger aria-label="Auto lock timeout" className="w-32">
            <SelectValue>
              {availableAutoLockOptions.find(({ value }) => value === autoLockOption?.value)?.label}
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            {availableAutoLockOptions.map(option => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </ConfigRow>
      <ConfigRow
        title="Paranoia mode"
        description="Do not keep the master key in memory after unlocking."
      >
        <Switch
          checked={currentConfig?.paranoiaMode ?? false}
          onCheckedChange={paranoiaMode => setConfig.mutate({ paranoiaMode })}
          disabled={disabled}
        />
      </ConfigRow>
      {setConfig.isError && (
        <p className="pt-4 text-sm text-destructive" role="alert">
          Configuration could not be saved.
        </p>
      )}
    </div>
  );
};
