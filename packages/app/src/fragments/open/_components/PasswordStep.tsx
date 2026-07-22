import { Button } from '@/components/button';
import { Field } from '@/components/field';
import { Input } from '@/components/input';
import { Label } from '@/components/label';
import { IconArrowRight, IconLoaderCircle } from '@/icons';
import { SetupLayout } from './SetupLayout';
import { StepError } from './StepError';
import type { RefObject, SubmitEvent } from 'react';

export type PasswordStepProps = {
  isPending: boolean;
  error?: string;
  hasNativePasswordInput: boolean;
  passwordRef: RefObject<HTMLInputElement | null>;
  onSubmit: (event: SubmitEvent<HTMLFormElement>) => void;
};

export const PasswordStep = ({
  mode,
  isPending,
  error,
  hasNativePasswordInput,
  passwordRef,
  onSubmit,
}: PasswordStepProps & { mode: 'create' | 'unlock' }) => {
  const isCreate = mode === 'create';

  return (
    <SetupLayout
      title={isCreate ? 'Create database' : 'Unlock database'}
      description={
        isCreate
          ? 'Choose the master password for this database.'
          : 'Enter the master password for this database.'
      }
    >
      <form onSubmit={onSubmit} className="space-y-4">
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
                autoComplete={isCreate ? 'new-password' : 'current-password'}
                disabled={isPending}
                required
              />

              <Button
                type="submit"
                className="w-10 h-10"
                size="icon-lg"
                variant="contrast"
                disabled={isPending}
              >
                {isPending ? <IconLoaderCircle className="animate-spin" /> : <IconArrowRight />}
              </Button>
            </Field>
          </div>
        )}
        {hasNativePasswordInput && (
          <Button type="submit" size="lg" disabled={isPending}>
            {isPending && <IconLoaderCircle className="animate-spin" />}
            Enter master password
          </Button>
        )}
        <StepError error={error} />
      </form>
    </SetupLayout>
  );
};
