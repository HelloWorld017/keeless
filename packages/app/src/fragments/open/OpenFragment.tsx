import BackgroundImage from '@/assets/images/background.webp?url';
import { useHost, useHostsLoading } from '@/fragments/_providers/HostProvider';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import { buildRoute, getRoute } from '@/utils/route';
import { useEffect, useState } from 'react';
import { Redirect, Route, Switch } from 'wouter';
import { useNavigate } from '../_providers/RouterProvider';
import { CheckingStep } from './_components/CheckingStep';
import { SelectStep } from './_components/SelectStep';
import { errorMessage } from './_utils/errorMessage';
import { OpenCreateFragment } from './create';
import { OpenStorageFragment } from './storage';
import { OpenUnlockFragment } from './unlock';

export const OpenFragment = () => {
  const host = useHost();
  const hostsLoading = useHostsLoading();
  const requestClient = useRequestClient();
  const client = requestClient.data;
  const navigate = useNavigate();
  const [isChecking, setIsChecking] = useState(false);
  const [checkingError, setCheckingError] = useState<string>();
  const [checkingAttempt, setCheckingAttempt] = useState(0);

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
  }, [checkingAttempt, client, navigate]);

  const isCheckingStepVisible =
    hostsLoading ||
    (Boolean(host) &&
      (requestClient.isPending || requestClient.isError || isChecking || checkingError));

  const checkingPending = hostsLoading || requestClient.isFetching || isChecking;
  const checkingFailure =
    checkingError ?? (requestClient.isError ? errorMessage(requestClient.error) : undefined);

  return (
    <div className="flex h-dvh items-center">
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
          <Switch>
            <Route path={getRoute('openStorage')} component={OpenStorageFragment} />
            <Route path={getRoute('openCreate')} component={OpenCreateFragment} />
            <Route path={getRoute('openUnlock')} component={OpenUnlockFragment} />
            <Route path={getRoute('open')} component={SelectStep} />
            <Redirect to={getRoute('open')} replace />
          </Switch>
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
