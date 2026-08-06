import { FieldCopyButton } from './FieldCopyButton';
import { FieldNote } from './FieldNote';
import { FieldPassword } from './FieldPassword';
import { FieldPlain } from './FieldPlain';
import { FieldTotp } from './FieldTotp';
import { PopoutFieldValue } from './PopoutFieldValue';
import type { DatabaseNodeId, EntryFieldInformation, FieldControl } from '@keeless/schema';

type EntryField = Extract<EntryFieldInformation, { type: 'field' }>;

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
  field: EntryField;
  label: string;
  control?: FieldControl | null;
}) => {
  if (field.kind === 'custom' && field.name === 'OTP' && field.isProtected && field.fieldId) {
    return <FieldTotp entryId={entryId} fieldId={field.fieldId} name={label} />;
  }
  if (control?.type === 'popout') {
    return <PopoutFieldValue field={field} label={label} />;
  }
  if (field.fieldId !== null && field.isProtected) {
    if (
      (!control && field.kind === 'notes') ||
      control?.type === 'richText' ||
      (control?.type === 'text' && control.lines > 1)
    ) {
      return <FieldNote fieldId={field.fieldId} name={label} value={field.value} isProtected />;
    }
    return <FieldPassword fieldId={field.fieldId} name={label} />;
  }
  if (control?.type === 'url') {
    const href = safeUrl(field.value);
    if (href) {
      return (
        <div className="space-y-1 px-4 py-3">
          <dt className="text-xs text-muted-foreground">{label}</dt>
          <dd className="flex min-w-0 items-start gap-2 text-sm">
            <a
              className="min-w-0 flex-1 break-words underline underline-offset-4"
              href={href}
              target="_blank"
              rel="noreferrer"
            >
              {field.value}
            </a>
            <FieldCopyButton label={label} value={field.value} />
          </dd>
        </div>
      );
    }
  }
  if (control?.type === 'checkbox') {
    return (
      <div className="flex items-center justify-between gap-3 px-4 py-3 text-sm">
        <dt className="text-muted-foreground">{label}</dt>
        <dd className="flex items-center gap-2">
          <input type="checkbox" checked={field.value === 'True'} disabled aria-label={label} />
          <FieldCopyButton label={label} value={field.value} />
        </dd>
      </div>
    );
  }
  if (
    (!control && field.kind === 'notes') ||
    control?.type === 'richText' ||
    (control?.type === 'text' && control.lines > 1)
  ) {
    return (
      <FieldNote
        fieldId={field.fieldId ?? ''}
        name={label}
        value={field.value}
        isProtected={false}
      />
    );
  }
  return <FieldPlain name={label} value={field.value} />;
};
