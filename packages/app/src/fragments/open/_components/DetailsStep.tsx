import { Button } from '@/components/button';
import { Input } from '@/components/input';
import { Label } from '@/components/label';
import { IconChevronLeft, IconLoaderCircle } from '@/icons';
import { SetupLayout } from './SetupLayout';
import { StepError } from './StepError';
import type { ComponentProps, SubmitEvent } from 'react';

// FIXME AI slop, get input component from host
export const DetailsStep = ({
  isPending,
  error,
  onBack,
  onSubmit,
}: {
  isPending: boolean;
  error?: string;
  onBack: () => void;
  onSubmit: (event: SubmitEvent<HTMLFormElement>) => void;
}) => (
  <SetupLayout title="Connect WebDAV" description="Enter the connection details for your server.">
    <form className="space-y-4" onSubmit={onSubmit}>
      <FormInput label="URL" name="url" type="url" autoComplete="url" required />
      <FormInput label="User" name="username" autoComplete="username" required />
      <FormInput
        label="Password"
        name="password"
        type="password"
        autoComplete="current-password"
        required
      />
      <FormInput label="Path (optional)" name="path" placeholder="keeless.kdbx" />
      <StepError error={error} />
      <div className="flex justify-between gap-3 pt-2">
        <Button type="button" variant="outline" disabled={isPending} onClick={onBack}>
          <IconChevronLeft /> Back
        </Button>
        <Button type="submit" disabled={isPending}>
          {isPending && <IconLoaderCircle className="animate-spin" />}
          Continue
        </Button>
      </div>
    </form>
  </SetupLayout>
);

const FormInput = ({ label, ...props }: { label: string } & ComponentProps<'input'>) => (
  <div className="space-y-2">
    <Label htmlFor={props.name}>{label}</Label>
    <Input id={props.name} {...props} />
  </div>
);
