import { Button } from '@/components/button';
import { Field } from '@/components/field';
import { Input } from '@/components/input';
import { Label } from '@/components/label';
import { useHasNativePasswordInput } from '@/fragments/_providers/HostProvider';
import { useRequestClient, useRequestMutation } from '@/fragments/_providers/QueryProvider';
import { IconArrowRight, IconLoaderCircle } from '@/icons';
import { CoreRequestError } from '@/utils/request';
import { buildRoute } from '@/utils/route';
import { useRef, useState } from 'react';
import { useNavigate } from '../../_providers/RouterProvider';
import { SetupLayout } from '../_components/SetupLayout';
import { StepError } from '../_components/StepError';
import { errorMessage } from '../_utils/errorMessage';
import type { OpenStepChange } from '../_types/Step';
import type { SubmitEvent } from 'react';

export const OpenCreateFragment = ({
  onStepChange,
  onBack,
}: {
  onStepChange: OpenStepChange;
  onBack: () => void;
}) => {
  const hasNativePasswordInput = useHasNativePasswordInput();
  const requestClient = useRequestClient();
  const navigate = useNavigate();
  const passwordRef = useRef<HTMLInputElement>(null);
  const operationPendingRef = useRef(false);
  const [isPending, setIsPending] = useState(false);
  const [error, setError] = useState<string>();
  const isDisabled = isPending || !requestClient.data || requestClient.isPending;

  const create = useRequestMutation('create');
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
        await create.mutateAsync({});
      } else {
        const input = event.currentTarget.elements.namedItem('master-password');
        if (!(input instanceof HTMLInputElement) || !input.value) {
          setError('Enter the master password for this database.');
          passwordRef.current?.focus();
          return;
        }
        const password = input.value;
        input.value = '';
        await create.mutateAsync({ password });
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
      if (nextError instanceof CoreRequestError && nextError.code === 'database_already_exists') {
        try {
          const { database } = await requestClient.data.request('getCoreStatus', {});
          if (database === 'unlocked') {
            await requestClient.data.upgrade();
            navigate(buildRoute('database'), { replace: true });
          } else {
            onStepChange({ kind: database === 'locked' ? 'unlock' : 'create' }, { replace: true });
          }
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

  return (
    <SetupLayout
      title="Create database"
      description="Choose the master password for this database."
      onBack={onBack}
    >
      <form onSubmit={event => void submit(event)} className="space-y-4">
        {!hasNativePasswordInput && (
          <div className="space-y-2">
            <Label htmlFor="master-password">Master password</Label>
            <Field orientation="horizontal" className="mt-4">
              <Input
                ref={passwordRef}
                id="master-password"
                name="master-password"
                type="password"
                className="h-10"
                autoComplete="new-password"
                disabled={isDisabled}
                required
              />
              <Button
                type="submit"
                className="w-10 h-10"
                size="icon-lg"
                variant="contrast"
                disabled={isDisabled}
              >
                {isPending ? <IconLoaderCircle className="animate-spin" /> : <IconArrowRight />}
              </Button>
            </Field>
          </div>
        )}
        {hasNativePasswordInput && (
          <Button type="submit" size="lg" disabled={isDisabled}>
            {isPending && <IconLoaderCircle className="animate-spin" />}
            Enter master password
          </Button>
        )}
        <StepError
          error={error ?? (requestClient.isError ? errorMessage(requestClient.error) : undefined)}
        />
      </form>
    </SetupLayout>
  );
};
