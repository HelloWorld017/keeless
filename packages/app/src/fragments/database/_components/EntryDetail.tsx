import { Alert, AlertAction, AlertDescription, AlertTitle } from '@/components/alert';
import { Button } from '@/components/button';
import { Input } from '@/components/input';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/sheet';
import { Skeleton } from '@/components/skeleton';
import { usePasswordInput } from '@/fragments/_providers/HostProvider';
import { useRequest, useRequestClient } from '@/fragments/_providers/QueryProvider';
import { useDebouncedValue } from '@/hooks/useDebouncedValue';
import {
  IconAlertCircle,
  IconChevronLeft,
  IconEye,
  IconEyeOff,
  IconLoaderCircle,
  IconX,
} from '@/icons';
import { cn } from '@/utils/css';
import { CoreRequestError } from '@/utils/request';
import { useEffect, useRef, useState, type ReactNode, type SubmitEvent } from 'react';
import { getEntryTitle } from './EntryItem';
import { ItemIcon } from './ItemIcon';
import type {
  EntryAttachmentInformation,
  EntryDetailResult,
  EntryFieldInformation,
  EntrySummary,
} from '@keeless/schema';

const dateFormatter = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'short',
});

const numberFormatter = new Intl.NumberFormat();

const formatDate = (timestamp: number | null) =>
  timestamp === null ? 'Unknown' : dateFormatter.format(new Date(timestamp));

const formatBytes = (size: number) => {
  if (size < 1024) {
    return `${numberFormatter.format(size)} B`;
  }

  const units = ['KB', 'MB', 'GB', 'TB'];
  let value = size / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value)} ${units[unitIndex]}`;
};

const EntryDetailSkeleton = ({ pending }: { pending: boolean }) => {
  const visible = useDebouncedValue(pending, 150, false);

  if (!visible) {
    return null;
  }

  return (
    <div
      className={cn(
        'animate-in w-full max-w-3xl space-y-8 p-6 duration-200 fade-in',
        !pending && 'animate-out opacity-0 fade-out',
      )}
      aria-label="Loading entry details"
    >
      <div className="space-y-3">
        <Skeleton className="h-5 w-20" />
        <Skeleton className="h-20 w-full" />
        <Skeleton className="h-20 w-full" />
        <Skeleton className="h-20 w-full" />
      </div>
      <div className="space-y-3">
        <Skeleton className="h-5 w-24" />
        <Skeleton className="h-32 w-full" />
      </div>
    </div>
  );
};

const DetailSection = ({ title, children }: { title: string; children: ReactNode }) => (
  <section className="space-y-3">
    <h2 className="text-sm font-semibold">{title}</h2>
    {children}
  </section>
);

const revealErrorMessage = (error: unknown) => {
  if (error instanceof CoreRequestError && error.code === 'invalid_credentials') {
    return 'The master password is incorrect.';
  }
  return 'The protected value could not be revealed.';
};

const PasswordPrompt = ({
  open,
  pending,
  error,
  onOpenChange,
  onSubmit,
}: {
  open: boolean;
  pending: boolean;
  error?: string;
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
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="bottom" className="mx-auto rounded-t-2xl p-4 sm:max-w-md sm:p-6">
        <SheetHeader className="p-0">
          <SheetTitle>Reveal protected value</SheetTitle>
          <SheetDescription>Enter the master password for this database.</SheetDescription>
        </SheetHeader>
        <form
          className="space-y-3"
          onSubmit={(event: SubmitEvent<HTMLFormElement>) => {
            event.preventDefault();
            const input = event.currentTarget.elements.namedItem('reveal-password');
            if (input instanceof HTMLInputElement && input.value) {
              const password = input.value;
              input.value = '';
              onSubmit(password);
            }
          }}
        >
          <Input
            ref={inputRef}
            name="reveal-password"
            type="password"
            autoComplete="current-password"
            disabled={pending}
            aria-invalid={Boolean(error)}
            required
          />
          {error && (
            <p className="text-sm text-destructive" role="alert">
              {error}
            </p>
          )}
          <Button type="submit" className="w-full" disabled={pending}>
            {pending && <IconLoaderCircle className="animate-spin" />}
            Reveal
          </Button>
        </form>
      </SheetContent>
    </Sheet>
  );
};

const EntryField = ({
  entryId,
  field,
}: {
  entryId: EntryDetailResult['id'];
  field: EntryFieldInformation;
}) => {
  const requestClient = useRequestClient();
  const onPasswordInput = usePasswordInput();
  const [revealedValue, setRevealedValue] = useState<string>();
  const [pending, setPending] = useState(false);
  const [promptOpen, setPromptOpen] = useState(false);
  const [error, setError] = useState<string>();
  const revealed = revealedValue !== undefined;
  const reveal = (password?: string) =>
    requestClient.data!.request('revealEntryField', {
      entryId,
      fieldIndex: field.fieldIndex,
      password,
    });
  const showValue = async () => {
    setPending(true);
    setError(undefined);
    try {
      const result = await reveal();
      setRevealedValue(result.value);
    } catch (nextError) {
      if (nextError instanceof CoreRequestError && nextError.code === 'password_required') {
        if (onPasswordInput) {
          const password = await onPasswordInput('reveal');
          if (password) {
            try {
              const result = await reveal(password);
              setRevealedValue(result.value);
            } catch (passwordError) {
              setError(revealErrorMessage(passwordError));
            }
          }
        } else {
          setPromptOpen(true);
        }
      } else {
        setError(revealErrorMessage(nextError));
      }
    } finally {
      setPending(false);
    }
  };
  const submitPassword = async (password: string) => {
    setPending(true);
    setError(undefined);
    try {
      const result = await reveal(password);
      setRevealedValue(result.value);
      setPromptOpen(false);
    } catch (nextError) {
      setError(revealErrorMessage(nextError));
    } finally {
      setPending(false);
    }
  };

  return (
    <div className="space-y-1 px-4 py-3">
      <dt className="text-xs text-muted-foreground">{field.name || 'Untitled field'}</dt>
      <dd className="flex min-w-0 items-start gap-2 text-sm">
        <span
          className={cn(
            'min-w-0 flex-1 whitespace-pre-wrap break-words',
            ((!field.isProtected && !field.value) || (revealed && !revealedValue)) &&
              'text-muted-foreground',
          )}
        >
          {field.isProtected && !revealed ? (
            <>
              <span className="tracking-[0.2em]" aria-hidden="true">
                ●●●●●●●●●●●
              </span>
              <span className="sr-only">Protected value</span>
            </>
          ) : (
            (revealedValue ?? field.value ?? 'Empty')
          )}
        </span>
        {field.isProtected && (
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="-my-1 shrink-0 text-muted-foreground"
            aria-label={revealed ? `Hide ${field.name}` : `Reveal ${field.name}`}
            aria-pressed={revealed}
            disabled={pending}
            onClick={() => {
              if (revealed) {
                setRevealedValue(undefined);
                setError(undefined);
              } else {
                void showValue();
              }
            }}
          >
            {pending ? (
              <IconLoaderCircle className="animate-spin" />
            ) : revealed ? (
              <IconEyeOff />
            ) : (
              <IconEye />
            )}
          </Button>
        )}
      </dd>
      {error && !promptOpen && (
        <p className="text-xs text-destructive" role="alert">
          {error}
        </p>
      )}
      {field.isProtected && (
        <PasswordPrompt
          open={promptOpen}
          pending={pending}
          error={error}
          onOpenChange={open => {
            setPromptOpen(open);
            if (!open) {
              setError(undefined);
            }
          }}
          onSubmit={password => void submitPassword(password)}
        />
      )}
    </div>
  );
};

const Attachment = ({ attachment }: { attachment: EntryAttachmentInformation }) => (
  <div className="flex items-center justify-between gap-4 px-4 py-3 text-sm">
    <span className="min-w-0 truncate">{attachment.name || 'Untitled attachment'}</span>
    <span className="shrink-0 text-muted-foreground">
      {attachment.isProtected && 'Protected · '}
      {formatBytes(attachment.size)}
    </span>
  </div>
);

const MetadataRow = ({ label, value }: { label: string; value: ReactNode }) => (
  <div className="grid gap-1 px-4 py-3 text-sm xl:grid-cols-[10rem_1fr] xl:gap-4">
    <dt className="text-muted-foreground">{label}</dt>
    <dd className="min-w-0 break-words xl:text-right">{value}</dd>
  </div>
);

const DetailContent = ({ detail }: { detail: EntryDetailResult }) => {
  const colorRows = [
    detail.backgroundColor && ['Background color', detail.backgroundColor],
    detail.foregroundColor && ['Foreground color', detail.foregroundColor],
  ].filter((row): row is string[] => Boolean(row));

  return (
    <div className="w-full max-w-3xl space-y-8 p-4 sm:p-6">
      <DetailSection title="Fields">
        {detail.fields.length > 0 ? (
          <dl className="divide-y rounded-lg border">
            {detail.fields.map((field, index) => (
              <EntryField
                key={`${detail.id}:${field.fieldIndex}:${index}`}
                entryId={detail.id}
                field={field}
              />
            ))}
          </dl>
        ) : (
          <p className="text-sm text-muted-foreground">No fields</p>
        )}
      </DetailSection>

      {detail.tags.length > 0 && (
        <DetailSection title="Tags">
          <p className="text-sm leading-6">{detail.tags.join(', ')}</p>
        </DetailSection>
      )}

      {detail.attachments.length > 0 && (
        <DetailSection title="Attachments">
          <div className="divide-y rounded-lg border">
            {detail.attachments.map((attachment, index) => (
              <Attachment key={`${attachment.name}:${index}`} attachment={attachment} />
            ))}
          </div>
        </DetailSection>
      )}

      <DetailSection title="Details">
        <dl className="divide-y rounded-lg border">
          <MetadataRow label="Created" value={formatDate(detail.creationTimeMs)} />
          <MetadataRow label="Modified" value={formatDate(detail.lastModificationTimeMs)} />
          <MetadataRow
            label="Expires"
            value={detail.expires ? formatDate(detail.expiryTimeMs) : 'Never'}
          />
          {detail.overrideUrl && <MetadataRow label="Override URL" value={detail.overrideUrl} />}
          {colorRows.map(([label, value]) => (
            <MetadataRow key={label} label={label} value={value} />
          ))}
        </dl>
      </DetailSection>
    </div>
  );
};

const EntryDetailQuery = ({ entry }: { entry: EntrySummary }) => {
  const detail = useRequest('getEntryDetail', { entryId: entry.id });

  if (detail.isPending) {
    return <EntryDetailSkeleton pending={detail.isPending} />;
  }

  if (detail.isError) {
    return (
      <Alert variant="destructive" className="m-4 w-auto max-w-3xl sm:m-6">
        <IconAlertCircle />
        <AlertTitle>Entry details could not be loaded</AlertTitle>
        <AlertDescription>Try loading the entry again.</AlertDescription>
        <AlertAction>
          <Button variant="outline" size="sm" onClick={() => detail.refetch()}>
            Retry
          </Button>
        </AlertAction>
      </Alert>
    );
  }

  return <DetailContent detail={detail.data} />;
};

export const EntryDetail = ({
  entry,
  selected,
  listPending,
  onClose,
}: {
  entry?: EntrySummary;
  selected: boolean;
  listPending: boolean;
  onClose: () => void;
}) => (
  <section
    className={cn(
      'min-h-0 min-w-0 flex-3 flex-col p-4 xl:p-6 xl:py-8',
      selected ? 'flex' : 'hidden md:flex',
    )}
    aria-labelledby={selected ? 'entry-detail-title' : undefined}
  >
    {selected ? (
      <div className="flex flex-col flex-1 max-w-180 mx-auto w-full min-h-0">
        <header className="flex items-center px-4 md:px-6 pb-2 md:pb-6">
          <Button
            variant="ghost"
            size="icon-lg"
            className="md:hidden mr-4"
            aria-label="Back to entries"
            onClick={onClose}
          >
            <IconChevronLeft />
          </Button>
          <div className="flex 2xl:justify-center items-center gap-3 test-xl md:text-2xl lg:text-3xl">
            {entry && (
              <ItemIcon
                icon={entry.icon}
                fallback="entry"
                className="hidden shrink-0 text-muted-foreground sm:block"
              />
            )}
            <h1 id="entry-detail-title" className="min-w-0 shrink-1 truncate font-semibold">
              {entry ? getEntryTitle(entry) : 'Entry details'}
            </h1>
          </div>
          <Button
            variant="ghost"
            size="icon-lg"
            className="absolute right-7 sm:right-8 hidden md:inline-flex"
            aria-label="Close entry details"
            onClick={onClose}
          >
            <IconX />
          </Button>
        </header>
        <div className="min-h-0 overflow-auto">
          {entry ? (
            <EntryDetailQuery entry={entry} />
          ) : listPending ? (
            <EntryDetailSkeleton pending={listPending} />
          ) : (
            <p className="p-6 text-center text-sm text-muted-foreground">Entry does not exist</p>
          )}
        </div>
      </div>
    ) : (
      <div className="flex flex-1 items-center justify-center p-6">
        <p className="text-sm text-muted-foreground">Select an entry to view its details</p>
      </div>
    )}
  </section>
);
