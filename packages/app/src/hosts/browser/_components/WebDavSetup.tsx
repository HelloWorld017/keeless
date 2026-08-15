import { Button } from '@/components/button';
import { Input } from '@/components/input';
import { Label } from '@/components/label';
import { StepError } from '@/fragments/open/_components/StepError';
import { IconLoaderCircle } from '@/icons';
import type { StorageSetupComponentProps } from '@/types/Host';
import type { ComponentProps, SubmitEvent } from 'react';

const formString = (data: FormData, name: string) => {
  const value = data.get(name);
  return typeof value === 'string' ? value : '';
};

const buildStoragePath = (endpoint: string, username: string, password: string, path: string) => {
  const url = new URL(endpoint);
  if (!['http:', 'https:'].includes(url.protocol)) {
    throw new Error('WebDAV URL must use HTTP or HTTPS.');
  }
  if (url.username || url.password || url.search || url.hash) {
    throw new Error('WebDAV URL must not include credentials, a query, or a fragment.');
  }
  const segments = path.split('/').filter(Boolean);
  if (segments.some(segment => segment === '.' || segment === '..')) {
    throw new Error('WebDAV path must not contain . or .. segments.');
  }
  const basePath = url.pathname.endsWith('/') ? url.pathname : `${url.pathname}/`;
  url.pathname = `${basePath}${segments.map(encodeURIComponent).join('/')}`;
  url.username = username;
  url.password = password;
  return url.href;
};

export const WebDavSetup = ({ isPending, error, onOpen }: StorageSetupComponentProps) => {
  const submit = (event: SubmitEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const endpoint = formString(data, 'url').trim();
    const username = formString(data, 'username').trim();
    let password = formString(data, 'password');
    const path = formString(data, 'path').trim();
    const passwordInput = form.elements.namedItem('password');
    if (passwordInput instanceof HTMLInputElement) {
      passwordInput.value = '';
    }
    void onOpen(async () => {
      try {
        return { provider: 'webdav', path: buildStoragePath(endpoint, username, password, path) };
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
      <div className="flex justify-end gap-3 pt-2">
        <Button type="submit" variant="contrast" disabled={isPending}>
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
