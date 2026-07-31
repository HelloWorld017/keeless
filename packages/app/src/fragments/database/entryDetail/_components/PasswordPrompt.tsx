import { Button } from '@/components/button';
import { Input } from '@/components/input';
import {
  AdaptiveSheet,
  AdaptiveSheetContent,
  AdaptiveSheetDescription,
  AdaptiveSheetHeader,
  AdaptiveSheetOverlay,
  AdaptiveSheetTitle,
} from '@/fragments/_components/AdaptiveSheet';
import { IconLoaderCircle } from '@/icons';
import { useEffect, useRef, type SubmitEvent } from 'react';

export const PasswordPrompt = ({
  open,
  pending,
  error,
  title,
  description,
  action,
  onOpenChange,
  onSubmit,
}: {
  open: boolean;
  pending: boolean;
  error?: string;
  title: string;
  description: string;
  action: string;
  onOpenChange: (open: boolean) => void;
  onSubmit: (password: string) => void;
}) => {
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) {
      return undefined;
    }
    const frame = requestAnimationFrame(() => inputRef.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, [open]);

  return (
    <AdaptiveSheet open={open} onOpenChange={onOpenChange}>
      <AdaptiveSheetOverlay forceRender />
      <AdaptiveSheetContent className="p-4 sm:p-6">
        <AdaptiveSheetHeader className="p-0">
          <AdaptiveSheetTitle>{title}</AdaptiveSheetTitle>
          <AdaptiveSheetDescription>{description}</AdaptiveSheetDescription>
        </AdaptiveSheetHeader>
        <form
          className="space-y-3"
          onSubmit={(event: SubmitEvent<HTMLFormElement>) => {
            event.preventDefault();
            const input = event.currentTarget.elements.namedItem('master-password');
            if (input instanceof HTMLInputElement && input.value) {
              const password = input.value;
              input.value = '';
              onSubmit(password);
            }
          }}
        >
          <Input
            ref={inputRef}
            name="master-password"
            type="password"
            autoComplete="current-password"
            aria-label="Master password"
            aria-invalid={Boolean(error)}
            aria-describedby={error ? 'password-prompt-error' : undefined}
            disabled={pending}
            required
          />
          {error && (
            <p id="password-prompt-error" className="text-sm text-destructive" role="alert">
              {error}
            </p>
          )}
          <Button type="submit" className="w-full" disabled={pending}>
            {pending && <IconLoaderCircle className="animate-spin" />}
            {action}
          </Button>
        </form>
      </AdaptiveSheetContent>
    </AdaptiveSheet>
  );
};
