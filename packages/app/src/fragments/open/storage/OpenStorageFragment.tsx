import { useHost, useHostsLoading } from '@/fragments/_providers/HostProvider';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import { buildRoute, getRoute } from '@/utils/route';
import { useEffect, useRef, useState } from 'react';
import { useRoute } from 'wouter';
import { useHistoryBackUntil, useNavigate } from '../../_providers/RouterProvider';
import { SetupLayout } from '../_components/SetupLayout';
import { errorMessage } from '../_utils/errorMessage';
import type { StorageDescriptorGetter } from '@/types/Host';

export const OpenStorageFragment = () => {
  const host = useHost();
  const hostsLoading = useHostsLoading();
  const requestClient = useRequestClient();
  const navigate = useNavigate();
  const historyBackUntil = useHistoryBackUntil();
  const [, params] = useRoute<{ storage: string }>(getRoute('openStorage'));
  const operationPendingRef = useRef(false);
  const [isPending, setIsPending] = useState(false);
  const [error, setError] = useState<string>();
  const storage = host?.storages.find(candidate => candidate.kind === params?.storage);
  const StorageSetup = storage?.setup.component;

  useEffect(() => {
    if (!hostsLoading && !StorageSetup) {
      historyBackUntil(buildRoute('open'));
    }
  }, [StorageSetup, historyBackUntil, hostsLoading]);

  const openStorage = async (getDescriptor: StorageDescriptorGetter) => {
    if (!requestClient.data || operationPendingRef.current) {
      return;
    }

    operationPendingRef.current = true;
    setIsPending(true);
    setError(undefined);
    try {
      const descriptor = await getDescriptor();
      await requestClient.data.request('open', { storage: descriptor });
      const { database } = await requestClient.data.request('getCoreStatus', {});
      if (database === 'unlocked') {
        await requestClient.data.upgrade();
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

  if (!storage || !StorageSetup) {
    return null;
  }

  return (
    <SetupLayout
      title={storage.setup.title ?? storage.label}
      description={storage.setup.description ?? storage.description}
    >
      <StorageSetup
        isPending={isPending || requestClient.isPending || !requestClient.data}
        error={error ?? (requestClient.isError ? errorMessage(requestClient.error) : undefined)}
        onOpen={openStorage}
      />
    </SetupLayout>
  );
};
