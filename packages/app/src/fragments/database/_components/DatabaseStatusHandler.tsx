import { Button } from '@/components/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/dialog';
import { Field } from '@/components/field';
import { Input } from '@/components/input';
import { Label } from '@/components/label';
import { useHasNativePasswordInput } from '@/fragments/_providers/HostProvider';
import { useRequest, useRequestClient } from '@/fragments/_providers/QueryProvider';
import { useNavigate } from '@/fragments/_providers/RouterProvider';
import { StepError } from '@/fragments/open/_components/StepError';
import { errorMessage } from '@/fragments/open/_utils/errorMessage';
import { IconArrowRight, IconLoaderCircle } from '@/icons';
import { CoreRequestError } from '@/utils/request';
import { buildRoute } from '@/utils/route';
import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import type { ReactNode, SubmitEvent } from 'react';

const DatabaseUnlockDialog = () => {
  const hasNativePasswordInput = useHasNativePasswordInput();
  const requestClient = useRequestClient();
  const queryClient = useQueryClient();
  const passwordRef = useRef<HTMLInputElement>(null);
  const operationPendingRef = useRef(false);
  const [isPending, setIsPending] = useState(false);
  const [error, setError] = useState<string>();
  const isDisabled = isPending || !requestClient.data || requestClient.isPending;

  const submit = async (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!requestClient.data || operationPendingRef.current) {
      return;
    }

    operationPendingRef.current = true;
    setIsPending(true);
    setError(undefined);
    try {
      if (hasNativePasswordInput) {
        await requestClient.data.request('unlock', {});
      } else {
        const input = event.currentTarget.elements.namedItem('master-password');
        if (!(input instanceof HTMLInputElement) || !input.value) {
          setError('Enter the master password for this database.');
          passwordRef.current?.focus();
          return;
        }
        const password = input.value;
        input.value = '';
        await requestClient.data.request('unlock', { password });
      }
      await requestClient.data.upgrade();
      await queryClient.invalidateQueries({ queryKey: ['request'] });
    } catch (nextError) {
      if (
        hasNativePasswordInput &&
        nextError instanceof CoreRequestError &&
        nextError.code === 'password_required'
      ) {
        return;
      }
      setError(errorMessage(nextError));
      requestAnimationFrame(() => passwordRef.current?.focus());
    } finally {
      operationPendingRef.current = false;
      setIsPending(false);
    }
  };

  return (
    <Dialog open onOpenChange={() => {}}>
      <DialogContent showCloseButton={false}>
        <DialogHeader>
          <DialogTitle>Database locked</DialogTitle>
          <DialogDescription>Enter the master password to continue.</DialogDescription>
        </DialogHeader>
        <form onSubmit={event => void submit(event)} className="space-y-4">
          {!hasNativePasswordInput && (
            <div className="space-y-2">
              <Label htmlFor="master-password">Master password</Label>
              <Field orientation="horizontal">
                <Input
                  ref={passwordRef}
                  id="master-password"
                  name="master-password"
                  type="password"
                  autoComplete="current-password"
                  disabled={isDisabled}
                  required
                />
                <Button type="submit" size="icon" variant="contrast" disabled={isDisabled}>
                  {isPending ? <IconLoaderCircle className="animate-spin" /> : <IconArrowRight />}
                  <span className="sr-only">Unlock database</span>
                </Button>
              </Field>
            </div>
          )}
          {hasNativePasswordInput && (
            <DialogFooter>
              <Button type="submit" disabled={isDisabled}>
                {isPending && <IconLoaderCircle className="animate-spin" />}
                Enter master password
              </Button>
            </DialogFooter>
          )}
          <StepError
            error={error ?? (requestClient.isError ? errorMessage(requestClient.error) : undefined)}
          />
        </form>
      </DialogContent>
    </Dialog>
  );
};

export const DatabaseStatusHandler = ({ children }: { children: ReactNode }) => {
  const navigate = useNavigate();
  const redirectPending = useRef(false);
  const coreStatus = useRequest(
    'getCoreStatus',
    {},
    {
      refetchInterval: 30_000,
      refetchOnWindowFocus: true,
    },
  );

  useEffect(() => {
    if (redirectPending.current) {
      return;
    }

    if (!coreStatus.isFetchedAfterMount) {
      return;
    }

    if (coreStatus.data?.database === 'not_exist') {
      redirectPending.current = true;
      navigate(buildRoute('open'), { replace: true });
    }
  }, [coreStatus.isFetchedAfterMount, coreStatus.data?.database, navigate]);

  if (!coreStatus.data) {
    return null;
  }

  if (coreStatus.data.database === 'not_exist') {
    return null;
  }

  return (
    <>
      {children}
      {coreStatus.data.database === 'locked' && <DatabaseUnlockDialog />}
    </>
  );
};
