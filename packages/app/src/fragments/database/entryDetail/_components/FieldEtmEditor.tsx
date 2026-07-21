import { Button } from '@/components/button';
import { Field, FieldError, FieldLabel } from '@/components/field';
import { Input } from '@/components/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/select';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/sheet';
import { useState } from 'react';
import { FieldNoteEditor } from './FieldNoteEditor';
import { FieldPasswordEditor } from './FieldPasswordEditor';
import { FieldPlainEditor } from './FieldPlainEditor';
import type { EntryPropertiesDraft } from '../_types/EntryPropertiesDraft';
import type { FieldDraft } from '../_types/FieldDraft';
import type { DatabaseNodeId, EtmLayoutItem } from '@keeless/schema';

const localInputValue = (value: number | null, type: 'date' | 'time' | 'datetime-local') => {
  if (value === null) {
    return '';
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return '';
  }
  const local = new Date(value - date.getTimezoneOffset() * 60_000).toISOString();
  if (type === 'date') {
    return local.slice(0, 10);
  }
  if (type === 'time') {
    return local.slice(11, 16);
  }
  return local.slice(0, 16);
};

const inputTimestamp = (
  value: string,
  type: 'date' | 'time' | 'datetime-local',
  current: number | null,
) => {
  if (!value) {
    return null;
  }
  let date: Date;
  if (type === 'time') {
    const [hours, minutes] = value.split(':').map(Number);
    date = current === null ? new Date() : new Date(current);
    date.setHours(hours, minutes, 0, 0);
  } else if (type === 'date') {
    const [year, month, day] = value.split('-').map(Number);
    date = current === null ? new Date(year, month - 1, day) : new Date(current);
    date.setFullYear(year, month - 1, day);
  } else {
    date = new Date(value);
  }
  return Number.isNaN(date.getTime()) ? null : date.getTime();
};

export const FieldEtmEditor = ({
  item,
  entryId,
  draft,
  properties,
  confirmation,
  confirmationInvalid,
  pending,
  onLoad,
  onChange,
  onPropertiesChange,
  onConfirmationChange,
}: {
  item: EtmLayoutItem;
  entryId: DatabaseNodeId;
  draft?: FieldDraft;
  properties: EntryPropertiesDraft;
  confirmation: string;
  confirmationInvalid: boolean;
  pending: boolean;
  onLoad: (value: string) => void;
  onChange: (patch: Partial<FieldDraft>) => void;
  onPropertiesChange: (patch: Partial<EntryPropertiesDraft>) => void;
  onConfirmationChange: (value: string) => void;
}) => {
  const [sheetOpen, setSheetOpen] = useState(false);
  const { target, control, label } = item;

  if (target.type === 'divider' || control.type === 'divider') {
    return (
      <div className="flex items-center gap-3 py-1 text-xs text-muted-foreground">
        <span className="h-px flex-1 bg-border" />
        {label && <span>{label}</span>}
        <span className="h-px flex-1 bg-border" />
      </div>
    );
  }
  if (target.type === 'passwordConfirmation') {
    return (
      <Field data-invalid={confirmationInvalid}>
        <FieldLabel htmlFor={`confirm-${target.passwordFieldId}`}>{label}</FieldLabel>
        <Input
          id={`confirm-${target.passwordFieldId}`}
          type="password"
          value={confirmation}
          aria-invalid={confirmationInvalid}
          disabled={pending}
          onChange={event => onConfirmationChange(event.target.value)}
        />
        {confirmationInvalid && <FieldError>Password confirmation does not match.</FieldError>}
      </Field>
    );
  }
  if (target.type === 'overrideUrl') {
    return (
      <Field>
        <FieldLabel htmlFor="etm-override-url">{label}</FieldLabel>
        <Input
          id="etm-override-url"
          type="url"
          value={properties.overrideUrl}
          disabled={pending}
          onChange={event => onPropertiesChange({ overrideUrl: event.target.value })}
        />
      </Field>
    );
  }
  if (target.type === 'tags') {
    return (
      <Field>
        <FieldLabel htmlFor="etm-tags">{label}</FieldLabel>
        <Input
          id="etm-tags"
          value={properties.tags}
          placeholder="Comma-separated tags"
          disabled={pending}
          onChange={event => onPropertiesChange({ tags: event.target.value, tagsChanged: true })}
        />
      </Field>
    );
  }
  if (target.type === 'expiry') {
    const type =
      control.type === 'time' ? 'time' : control.type === 'dateTime' ? 'datetime-local' : 'date';
    return (
      <Field>
        <div className="flex items-center justify-between gap-3">
          <FieldLabel htmlFor="etm-expiry">{label}</FieldLabel>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={properties.expires}
              disabled={pending}
              onChange={event =>
                onPropertiesChange({
                  expires: event.target.checked,
                  expiryTimeMs: event.target.checked
                    ? (properties.expiryTimeMs ?? Date.now())
                    : null,
                })
              }
            />
            Expires
          </label>
        </div>
        <Input
          id="etm-expiry"
          type={type}
          value={localInputValue(properties.expiryTimeMs, type)}
          disabled={pending || !properties.expires}
          onChange={event => {
            const expiryTimeMs = inputTimestamp(event.target.value, type, properties.expiryTimeMs);
            onPropertiesChange({ expiryTimeMs, expires: expiryTimeMs !== null });
          }}
        />
      </Field>
    );
  }
  if (target.type !== 'field' || !draft) {
    return null;
  }

  const id = `${draft.key}-value`;
  const value = draft.value ?? '';
  const existing = draft.fieldId !== null && draft.originalIsProtected && !draft.valueChanged;
  const requiresProtectedEditor = draft.originalIsProtected || draft.isProtected;
  const changeValue = (nextValue: string) => onChange({ value: nextValue, valueChanged: true });
  let editor;

  if (control.type === 'popout') {
    editor = (
      <>
        <Button
          id={id}
          type="button"
          variant="outline"
          disabled={pending}
          onClick={() => setSheetOpen(true)}
        >
          Open
        </Button>
        <Sheet open={sheetOpen} onOpenChange={setSheetOpen}>
          <SheetContent className="p-4 sm:max-w-xl sm:p-6">
            <SheetHeader className="p-0">
              <SheetTitle>{label}</SheetTitle>
              <SheetDescription>Edit the expanded field value.</SheetDescription>
            </SheetHeader>
            <FieldNoteEditor
              entryId={entryId}
              fieldId={draft.fieldId}
              name={label}
              value={value}
              isProtected={requiresProtectedEditor}
              existing={existing}
              disabled={pending}
              onLoad={onLoad}
              onChange={changeValue}
            />
          </SheetContent>
        </Sheet>
      </>
    );
  } else if (control.type === 'richText' || (control.type === 'text' && control.lines > 1)) {
    editor = (
      <FieldNoteEditor
        id={id}
        entryId={entryId}
        fieldId={draft.fieldId}
        name={label}
        value={value}
        isProtected={requiresProtectedEditor}
        existing={existing}
        lines={control.type === 'richText' ? control.lines : control.lines}
        disabled={pending}
        onLoad={onLoad}
        onChange={changeValue}
      />
    );
  } else if (requiresProtectedEditor) {
    editor = (
      <FieldPasswordEditor
        id={id}
        entryId={entryId}
        fieldId={draft.fieldId}
        name={label}
        value={value}
        existing={existing}
        disabled={pending}
        onLoad={onLoad}
        onChange={changeValue}
      />
    );
  } else if (control.type === 'checkbox') {
    editor = (
      <label className="flex items-center gap-2 text-sm">
        <input
          id={id}
          type="checkbox"
          checked={value === 'True'}
          disabled={pending}
          onChange={event => changeValue(event.target.checked ? 'True' : 'False')}
        />
        {value === 'True' ? 'True' : 'False'}
      </label>
    );
  } else if (control.type === 'select' && control.options.length > 0) {
    editor = (
      <Select
        value={value || null}
        onValueChange={next => next !== null && changeValue(next)}
        disabled={pending}
      >
        <SelectTrigger id={id} className="w-full">
          <SelectValue placeholder="Select a value" />
        </SelectTrigger>
        <SelectContent>
          {control.options.map(option => (
            <SelectItem key={option} value={option}>
              {option}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  } else {
    const type =
      control.type === 'url'
        ? 'url'
        : control.type === 'date'
          ? 'date'
          : control.type === 'time'
            ? 'time'
            : control.type === 'dateTime'
              ? 'datetime-local'
              : 'text';
    editor =
      type === 'text' ? (
        <FieldPlainEditor
          id={id}
          name={label}
          value={value}
          disabled={pending}
          onChange={changeValue}
        />
      ) : (
        <Input
          id={id}
          type={type}
          value={value}
          disabled={pending}
          onChange={event => changeValue(event.target.value)}
        />
      );
  }

  return (
    <Field>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      {editor}
    </Field>
  );
};
