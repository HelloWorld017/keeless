import { Input } from '@/components/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/select';
import { FieldNoteEditor } from './FieldNoteEditor';
import { FieldPasswordEditor } from './FieldPasswordEditor';
import { FieldPlainEditor } from './FieldPlainEditor';
import { PopoutFieldEditor } from './PopoutFieldEditor';
import type { FieldDraft } from '../_types/FieldDraft';
import type { DatabaseNodeId, FieldControl } from '@keeless/schema';

export const EntryFieldEditor = ({
  entryId,
  draft,
  id = `${draft.key}-value`,
  label,
  control,
  placeholder,
  pending,
  onLoad,
  onChange,
}: {
  entryId: DatabaseNodeId;
  draft: FieldDraft;
  id?: string;
  label: string;
  control?: FieldControl;
  placeholder?: string;
  pending: boolean;
  onLoad: (value: string) => void;
  onChange: (value: string) => void;
}) => {
  const value = draft.value ?? '';
  const existing = draft.fieldId !== null && draft.originalIsProtected && !draft.valueChanged;
  const protectedEditor = draft.originalIsProtected || draft.isProtected;

  if (control?.type === 'popout') {
    return (
      <PopoutFieldEditor
        entryId={entryId}
        draft={draft}
        id={id}
        label={label}
        pending={pending}
        onLoad={onLoad}
        onChange={onChange}
      />
    );
  }
  if (
    (!control && draft.kind === 'notes') ||
    control?.type === 'richText' ||
    (control?.type === 'text' && control.lines > 1)
  ) {
    return (
      <FieldNoteEditor
        id={id}
        entryId={entryId}
        fieldId={draft.fieldId}
        name={label}
        value={value}
        isProtected={protectedEditor}
        existing={existing}
        lines={control?.type === 'richText' || control?.type === 'text' ? control.lines : undefined}
        disabled={pending}
        onLoad={onLoad}
        onChange={onChange}
      />
    );
  }
  if (protectedEditor) {
    return (
      <FieldPasswordEditor
        id={id}
        entryId={entryId}
        fieldId={draft.fieldId}
        name={label}
        value={value}
        existing={existing}
        disabled={pending}
        onLoad={onLoad}
        onChange={onChange}
      />
    );
  }
  if (control?.type === 'checkbox') {
    return (
      <label className="flex items-center gap-2 text-sm">
        <input
          id={id}
          type="checkbox"
          checked={value === 'True'}
          disabled={pending}
          onChange={event => onChange(event.target.checked ? 'True' : 'False')}
        />
        {value === 'True' ? 'True' : 'False'}
      </label>
    );
  }
  if (control?.type === 'select' && control.options.length > 0) {
    return (
      <Select
        value={value || null}
        onValueChange={next => next !== null && onChange(next)}
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
  }

  const type =
    control?.type === 'url'
      ? 'url'
      : control?.type === 'date'
        ? 'date'
        : control?.type === 'time'
          ? 'time'
          : control?.type === 'dateTime'
            ? 'datetime-local'
            : 'text';
  return type === 'text' ? (
    <FieldPlainEditor
      id={id}
      name={label}
      value={value}
      placeholder={placeholder}
      disabled={pending}
      onChange={onChange}
    />
  ) : (
    <Input
      id={id}
      type={type}
      value={value}
      disabled={pending}
      onChange={event => onChange(event.target.value)}
    />
  );
};
