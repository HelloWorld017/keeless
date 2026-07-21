import { FieldNote } from './FieldNote';
import { FieldPassword } from './FieldPassword';
import { FieldPlain } from './FieldPlain';
import { PopoutFieldValue } from './PopoutFieldValue';
import type { DatabaseNodeId, EntryFieldInformation, FieldControl } from '@keeless/schema';

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

export const EntryFieldValue = ({
  entryId,
  field,
  label,
  control,
}: {
  entryId: DatabaseNodeId;
  field?: EntryFieldInformation;
  label: string;
  control?: FieldControl;
}) => {
  if (control?.type === 'popout') {
    return <PopoutFieldValue entryId={entryId} field={field} label={label} />;
  }
  if (field?.isProtected) {
    if (
      (!control && field.kind === 'notes') ||
      control?.type === 'richText' ||
      (control?.type === 'text' && control.lines > 1)
    ) {
      return (
        <FieldNote
          entryId={entryId}
          fieldId={field.fieldId}
          name={label}
          value={field.value}
          isProtected
        />
      );
    }
    return <FieldPassword entryId={entryId} fieldId={field.fieldId} name={label} />;
  }
  if (control?.type === 'url') {
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
  if (control?.type === 'checkbox') {
    return (
      <div className="flex items-center justify-between gap-3 px-4 py-3 text-sm">
        <dt className="text-muted-foreground">{label}</dt>
        <dd>
          <input type="checkbox" checked={field?.value === 'True'} disabled aria-label={label} />
        </dd>
      </div>
    );
  }
  if (
    (!control && field?.kind === 'notes') ||
    control?.type === 'richText' ||
    (control?.type === 'text' && control.lines > 1)
  ) {
    return (
      <FieldNote
        entryId={entryId}
        fieldId={field?.fieldId ?? ''}
        name={label}
        value={field?.value ?? null}
        isProtected={false}
      />
    );
  }
  return <FieldPlain name={label} value={field?.value ?? null} />;
};
