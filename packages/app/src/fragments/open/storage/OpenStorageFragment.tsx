import { useHost, useHostsLoading } from '@/fragments/_providers/HostProvider';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import { buildRoute } from '@/utils/route';
import { useEffect, useRef, useState } from 'react';
import { useNavigate } from '../../_providers/RouterProvider';
import { SetupLayout } from '../_components/SetupLayout';
import { errorMessage } from '../_utils/errorMessage';
import type { OpenStepChange } from '../_types/Step';
import type { StorageProviderGetter } from '@/types/Host';

export const OpenStorageFragment = ({
  storageKind,
  onStepChange,
  onBack,
}: {
  storageKind: string;
  onStepChange: OpenStepChange;
  onBack: () => void;
}) => {
  const host = useHost();
  const hostsLoading = useHostsLoading();
  const requestClient = useRequestClient();
  const navigate = useNavigate();
  const operationPendingRef = useRef(false);
  const [isPending, setIsPending] = useState(false);
  const [error, setError] = useState<string>();
  const storage = host?.storages.find(candidate => candidate.kind === storageKind);
  const StorageSetup = storage?.setup.component;

  useEffect(() => {
    if (!hostsLoading && !StorageSetup) {
      onStepChange({ kind: 'select' }, { replace: true });
    }
  }, [StorageSetup, hostsLoading, onStepChange]);

  const openStorage = async (getProvider: StorageProviderGetter) => {
    if (!requestClient.data || operationPendingRef.current) {
      return;
    }

    operationPendingRef.current = true;
    setIsPending(true);
    setError(undefined);
    try {
      await requestClient.data.request('open', { storage: await getProvider() });
      const { database } = await requestClient.data.request('getCoreStatus', {});
      if (database === 'unlocked') {
        await requestClient.data.upgrade();
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

  if (!storage || !StorageSetup) {
    return null;
  }

  return (
    <SetupLayout
      title={storage.setup.title ?? storage.label}
      description={storage.setup.description ?? storage.description}
      onBack={onBack}
    >
      <StorageSetup
        isPending={isPending || requestClient.isPending || !requestClient.data}
        error={error ?? (requestClient.isError ? errorMessage(requestClient.error) : undefined)}
        onOpen={openStorage}
      />
    </SetupLayout>
  );
};
