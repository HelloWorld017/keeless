import BackgroundImage from '@/assets/images/background.webp?url';
import { useHost, useHostsLoading } from '@/fragments/_providers/HostProvider';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import { invalidateByResource } from '@/utils/request';
import { buildRoute } from '@/utils/route';
import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { useNavigate } from '../_providers/RouterProvider';
import { CheckingStep } from './_components/CheckingStep';
import { RecentStep } from './_components/RecentStep';
import { errorMessage } from './_utils/errorMessage';
import { OpenCreateFragment } from './create';
import { OpenSelectFragment } from './select/OpenSelectFragment';
import { OpenStorageFragment } from './storage';
import { OpenUnlockFragment } from './unlock';
import type { OpenStep, OpenStepChange } from './_types/Step';

export const OpenFragment = () => {
  const host = useHost();
  const hostsLoading = useHostsLoading();
  const queryClient = useQueryClient();
  const requestClient = useRequestClient();
  const client = requestClient.data;
  const navigate = useNavigate();
  const [isChecking, setIsChecking] = useState(false);
  const [checkingError, setCheckingError] = useState<string>();
  const [checkingAttempt, setCheckingAttempt] = useState(0);
  const [steps, setSteps] = useState<OpenStep[]>([{ kind: 'recent' }]);
  const step = steps.at(-1);

  const changeStep: OpenStepChange = (nextStep, options) => {
    setSteps(current =>
      options?.replace ? [...current.slice(0, -1), nextStep] : [...current, nextStep],
    );
  };
  const backStep = () => {
    setSteps(current => (current.length > 1 ? current.slice(0, -1) : current));
  };

  useEffect(() => {
    if (!client) {
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
            await invalidateByResource(queryClient, ['databaseStatus']);
            navigate(buildRoute('database'), { replace: true });
          }
          return;
        }

        if (database === 'locked') {
          setSteps(current => {
            if (current.at(-1)?.kind === 'unlock') {
              return current;
            }
            return [...current.slice(0, -1), { kind: 'unlock' }];
          });
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
  }, [queryClient, checkingAttempt, client, navigate]);

  const isCheckingStepVisible =
    hostsLoading ||
    (Boolean(host) &&
      (requestClient.isPending || requestClient.isError || isChecking || checkingError));

  const checkingPending = hostsLoading || requestClient.isFetching || isChecking;
  const checkingFailure =
    checkingError ?? (requestClient.isError ? errorMessage(requestClient.error) : undefined);

  return (
    <div className="flex h-full items-center">
      <div className="flex-[0_0_auto] max-w-200 w-full">
        {isCheckingStepVisible ? (
          <CheckingStep
            isPending={checkingPending}
            error={checkingFailure}
            onRetry={() => {
              setCheckingError(undefined);
              if (requestClient.isError) {
                void requestClient.refetch();
                return;
              }
              setIsChecking(true);
              setCheckingAttempt(current => current + 1);
            }}
          />
        ) : (
          <>
            {step?.kind === 'recent' && <RecentStep onStepChange={changeStep} />}
            {step?.kind === 'select' && (
              <OpenSelectFragment onStepChange={changeStep} onBack={backStep} />
            )}
            {step?.kind === 'storage' && (
              <OpenStorageFragment
                storageKind={step.storage}
                onStepChange={changeStep}
                onBack={backStep}
              />
            )}
            {step?.kind === 'create' && (
              <OpenCreateFragment onStepChange={changeStep} onBack={backStep} />
            )}
            {step?.kind === 'unlock' && (
              <OpenUnlockFragment onStepChange={changeStep} onBack={backStep} />
            )}
          </>
        )}
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
};
