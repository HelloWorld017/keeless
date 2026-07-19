import { Button } from '@/components/button';
import { Input } from '@/components/input';
import { Label } from '@/components/label';
import { StepError } from '@/fragments/open/_components/StepError';
import { IconChevronLeft, IconLoaderCircle } from '@/icons';
import type { StorageSetupComponentProps } from '@/types/Host';
import type { BrowserCore } from '@keeless/host-browser';
import type { ComponentProps, SubmitEvent } from 'react';

const formString = (data: FormData, name: string) => {
  const value = data.get(name);
  return typeof value === 'string' ? value : '';
};

export const WebDavSetup = ({
  getCore,
  isPending,
  error,
  onBack,
  onOpen,
}: StorageSetupComponentProps & { getCore: () => BrowserCore }) => {
  const submit = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const url = formString(data, 'url').trim();
    const username = formString(data, 'username').trim();
    let password = formString(data, 'password');
    const path = formString(data, 'path').trim();
    const passwordInput = form.elements.namedItem('password');
    if (passwordInput instanceof HTMLInputElement) {
      passwordInput.value = '';
    }
    void onOpen(async () => {
      try {
        await getCore().configureWebDav(url, username, password);
        return { provider: 'webdav', path };
      } finally {
        password = '';
      }
    });
  };

  return (
    <form className="space-y-4" onSubmit={submit}>
      <FormInput label="URL" name="url" type="url" autoComplete="url" required />
      <FormInput label="User" name="username" autoComplete="username" required />
      <FormInput
        label="Password"
        name="password"
        type="password"
        autoComplete="current-password"
        required
      />
      <FormInput label="Path (optional)" name="path" />
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
  );
};

const FormInput = ({ label, ...props }: { label: string } & ComponentProps<'input'>) => (
  <div className="space-y-2">
    <Label htmlFor={props.name}>{label}</Label>
    <Input id={props.name} {...props} />
  </div>
);
