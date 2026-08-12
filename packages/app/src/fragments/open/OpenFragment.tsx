import BackgroundImage from '@/assets/images/background.webp?asset';
import {
  useHasNativePasswordInput,
  useHost,
  useHostOverride,
  useHosts,
  useHostsLoading,
  useSelectHost,
} from '@/fragments/_providers/HostProvider';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import { CoreRequestError } from '@/utils/request';
import { buildRoute } from '@/utils/route';
import { useEffect, useRef, useState } from 'react';
import { useNavigate } from '../_providers/RouterProvider';
import { CheckingStep } from './_components/CheckingStep';
import { CreateStep } from './_components/CreateStep';
import { SelectStep } from './_components/SelectStep';
import { SetupLayout } from './_components/SetupLayout';
import { UnlockStep } from './_components/UnlockStep';
import type { HostStorage, StorageDescriptorGetter } from '@/types/Host';
import type { DatabaseStatus } from '@keeless/schema';
import type { SubmitEvent } from 'react';

type SetupStep = 'select' | 'storage' | 'create' | 'unlock' | 'checking';
type SetupPasswordInputMode = 'create' | 'unlock';

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

const OpenFragmentContents = () => {
  const host = useHost();
  const hosts = useHosts();
  const hostsLoading = useHostsLoading();
  const isHostOverride = useHostOverride();
  const hasNativePasswordInput = useHasNativePasswordInput();
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
      .request('getCoreStatus', {})
      .then(({ database }) => {
        if (!active) {
          return;
        }
        if (database === 'unlocked') {
          void requestClient.data?.upgrade().then(() => {
            if (active) navigate(buildRoute('database'), { replace: true });
          });
        } else {
          setStep(database === 'locked' ? 'unlock' : 'select');
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
      moveFromStatus(database);
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
      setStep('storage');
      return;
    }
    void openStorage(nextStorage.setup.getDefaultDescriptor);
  };

  const requestPassword = (form: HTMLFormElement): string | null => {
    const input = form.elements.namedItem('master-password');
    if (!(input instanceof HTMLInputElement) || !input.value) {
      setError('Enter the master password for this database.');
      passwordRef.current?.focus();
      return null;
    }
    const password = input.value;
    input.value = '';
    return password;
  };

  const submitPassword = async (
    mode: SetupPasswordInputMode,
    event: SubmitEvent<HTMLFormElement>,
  ) => {
    event.preventDefault();
    if (!requestClient.data || operationPendingRef.current) {
      return;
    }
    operationPendingRef.current = true;
    setIsPending(true);
    setError(undefined);
    try {
      if (hasNativePasswordInput) {
        await requestClient.data.request(mode, {});
      } else {
        const password = requestPassword(event.currentTarget);
        if (!password) {
          return;
        }
        await requestClient.data.request(mode, { password });
      }
      await requestClient.data.upgrade();
      navigate(buildRoute('database'), { replace: true });
    } catch (nextError) {
      if (
        hasNativePasswordInput &&
        nextError instanceof CoreRequestError &&
        nextError.code === 'password_required'
      ) {
        return;
      }
      if (
        mode === 'create' &&
        nextError instanceof CoreRequestError &&
        nextError.code === 'database_already_exists'
      ) {
        try {
          const { database } = await requestClient.data.request('getCoreStatus', {});
          moveFromStatus(database);
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
          const { database } = await requestClient.data.request('getCoreStatus', {});
          moveFromStatus(database);
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

  if (step === 'checking') {
    const checkingPending = requestClient.isPending || isPending;
    const checkingError =
      error ?? (requestClient.isError ? errorMessage(requestClient.error) : undefined);
    return (
      <CheckingStep
        isPending={checkingPending}
        error={checkingError}
        onRetry={() => void requestClient.refetch()}
      />
    );
  }

  if (step === 'storage' && storage?.setup.component) {
    const StorageSetup = storage.setup.component;
    return (
      <SetupLayout
        title={storage.setup.title ?? storage.label}
        description={storage.setup.description ?? storage.description}
      >
        <StorageSetup
          isPending={isPending}
          error={error}
          onOpen={openStorage}
          onBack={() => {
            setError(undefined);
            setStep('select');
          }}
        />
      </SetupLayout>
    );
  }

  if (step === 'create') {
    return (
      <CreateStep
        isPending={isPending}
        error={error}
        hasNativePasswordInput={hasNativePasswordInput}
        passwordRef={passwordRef}
        onSubmit={event => void submitPassword('create', event)}
      />
    );
  }

  if (step === 'unlock') {
    return (
      <UnlockStep
        isPending={isPending}
        error={error}
        hasNativePasswordInput={hasNativePasswordInput}
        passwordRef={passwordRef}
        onSubmit={event => void submitPassword('unlock', event)}
      />
    );
  }

  return (
    <SelectStep
      host={host}
      hosts={hosts}
      storage={storage}
      hostsLoading={hostsLoading}
      isHostOverride={isHostOverride}
      isPending={isPending}
      isRequestReady={Boolean(requestClient.data)}
      requestError={requestClient.isError ? errorMessage(requestClient.error) : undefined}
      error={error}
      onSelectHost={value => {
        selectHost(value);
        setStorage(undefined);
        setError(undefined);
      }}
      onChooseStorage={chooseStorage}
      onRetryRequest={() => void requestClient.refetch()}
    />
  );
};

export const OpenFragment = () => (
  <div className="flex h-dvh items-center">
    <div className="flex-[0_0_auto] max-w-200 w-full">
      <OpenFragmentContents />
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
