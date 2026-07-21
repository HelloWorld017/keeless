import { Button } from '@/components/button';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/sheet';
import { cn } from '@/utils/css';
import { useState } from 'react';
import { useProtectedReveal } from '../_hooks/useProtectedReveal';
import { formatDate } from '../_utils/format';
import { FieldNote } from './FieldNote';
import { FieldPassword } from './FieldPassword';
import { FieldPlain } from './FieldPlain';
import type { EntryDetailResult, EntryFieldInformation, EtmLayoutItem } from '@keeless/schema';

const safeUrl = (value: string | null) => {
  if (!value) {
    return undefined;
  }
  try {
    const url = new URL(value);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : undefined;
  } catch {
    return undefined;
  }
};

const PopoutValue = ({
  detail,
  field,
  label,
}: {
  detail: EntryDetailResult;
  field?: EntryFieldInformation;
  label: string;
}) => {
  const [open, setOpen] = useState(false);
  const [protectedValue, setProtectedValue] = useState<string>();
  const isProtected = field?.isProtected ?? false;
  const { pending, error, revealed, toggleReveal, prompt, promptOpen } = useProtectedReveal({
    entryId: detail.id,
    fieldId: isProtected ? (field?.fieldId ?? null) : null,
    onReveal: setProtectedValue,
  });
  const value = isProtected ? protectedValue : field?.value;

  return (
    <div className="space-y-1 px-4 py-3">
      <dt className="text-xs text-muted-foreground">{label || 'Untitled field'}</dt>
      <dd>
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-label={`${isProtected && !revealed ? 'Reveal' : 'Open'} ${label || 'field'}`}
          disabled={pending}
          onClick={() => (isProtected && !revealed ? void toggleReveal() : setOpen(true))}
        >
          {isProtected && !revealed ? 'Reveal' : 'Open'}
        </Button>
      </dd>
      {error && !promptOpen && <p className="text-xs text-destructive">{error}</p>}
      {prompt}
      <Sheet
        open={open}
        onOpenChange={nextOpen => {
          setOpen(nextOpen);
          if (!nextOpen && isProtected && revealed) {
            void toggleReveal();
          }
        }}
      >
        <SheetContent className="p-4 sm:max-w-xl sm:p-6">
          <SheetHeader className="p-0">
            <SheetTitle>{label || 'Untitled field'}</SheetTitle>
            <SheetDescription>Expanded field value</SheetDescription>
          </SheetHeader>
          <div className={cn('whitespace-pre-wrap break-words', !value && 'text-muted-foreground')}>
            {value || 'Empty'}
          </div>
        </SheetContent>
      </Sheet>
    </div>
  );
};

export const FieldEtmView = ({
  item,
  detail,
  field,
}: {
  item: EtmLayoutItem;
  detail: EntryDetailResult;
  field?: EntryFieldInformation;
}) => {
  const { target, control, label } = item;

  if (target.type === 'passwordConfirmation') {
    return null;
  }
  if (target.type === 'divider' || control.type === 'divider') {
    return (
      <div className="px-4 py-3">
        <div className="flex items-center gap-3 text-xs text-muted-foreground">
          <span className="h-px flex-1 bg-border" />
          {label && <span>{label}</span>}
          <span className="h-px flex-1 bg-border" />
        </div>
      </div>
    );
  }
  if (target.type === 'overrideUrl') {
    const href = safeUrl(detail.overrideUrl);
    return href ? (
      <div className="space-y-1 px-4 py-3">
        <dt className="text-xs text-muted-foreground">{label}</dt>
        <dd className="min-w-0 break-words text-sm">
          <a className="underline underline-offset-4" href={href} target="_blank" rel="noreferrer">
            {detail.overrideUrl}
          </a>
        </dd>
      </div>
    ) : (
      <FieldPlain name={label} value={detail.overrideUrl} />
    );
  }
  if (target.type === 'expiry') {
    return (
      <FieldPlain name={label} value={detail.expires ? formatDate(detail.expiryTimeMs) : 'Never'} />
    );
  }
  if (target.type === 'tags') {
    return <FieldPlain name={label} value={detail.tags.join(', ')} />;
  }
  if (target.type !== 'field') {
    return null;
  }

  if (control.type === 'popout') {
    return <PopoutValue detail={detail} field={field} label={label} />;
  }
  if (field?.isProtected) {
    if (control.type === 'richText' || (control.type === 'text' && control.lines > 1)) {
      return (
        <FieldNote
          entryId={detail.id}
          fieldId={field.fieldId}
          name={label}
          value={field.value}
          isProtected
        />
      );
    }
    return <FieldPassword entryId={detail.id} fieldId={field.fieldId} name={label} />;
  }
  if (control.type === 'url') {
    const href = safeUrl(field?.value ?? null);
    if (href) {
      return (
        <div className="space-y-1 px-4 py-3">
          <dt className="text-xs text-muted-foreground">{label}</dt>
          <dd className="min-w-0 break-words text-sm">
            <a
              className="underline underline-offset-4"
              href={href}
              target="_blank"
              rel="noreferrer"
            >
              {field?.value}
            </a>
          </dd>
        </div>
      );
    }
  }
  if (control.type === 'checkbox') {
    return (
      <div className="flex items-center justify-between gap-3 px-4 py-3 text-sm">
        <dt className="text-muted-foreground">{label}</dt>
        <dd>
          <input type="checkbox" checked={field?.value === 'True'} disabled aria-label={label} />
        </dd>
      </div>
    );
  }
  if (control.type === 'richText' || (control.type === 'text' && control.lines > 1)) {
    return (
      <FieldNote
        entryId={detail.id}
        fieldId={field?.fieldId ?? ''}
        name={label}
        value={field?.value ?? null}
        isProtected={false}
      />
    );
  }
  return <FieldPlain name={label} value={field?.value ?? null} />;
};
