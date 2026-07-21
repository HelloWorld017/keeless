import { Field, FieldLabel } from '@/components/field';
import { Input } from '@/components/input';
import type { EntryPropertiesDraft } from '../_types/EntryPropertiesDraft';
import type { FieldControl } from '@keeless/schema';

const localInputValue = (value: number | null, type: 'date' | 'time' | 'datetime-local') => {
  if (value === null) {
    return '';
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return '';
  }
  const local = new Date(value - date.getTimezoneOffset() * 60_000).toISOString();
  return type === 'date'
    ? local.slice(0, 10)
    : type === 'time'
      ? local.slice(11, 16)
      : local.slice(0, 16);
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

export const ExpiryEditor = ({
  id,
  label,
  control,
  properties,
  pending,
  onChange,
}: {
  id: string;
  label: string;
  control: FieldControl;
  properties: EntryPropertiesDraft;
  pending: boolean;
  onChange: (patch: Partial<EntryPropertiesDraft>) => void;
}) => {
  const type =
    control.type === 'time' ? 'time' : control.type === 'dateTime' ? 'datetime-local' : 'date';
  return (
    <Field>
      <div className="flex items-center justify-between gap-3">
        <FieldLabel htmlFor={id}>{label}</FieldLabel>
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={properties.expires}
            disabled={pending}
            onChange={event =>
              onChange({
                expires: event.target.checked,
                expiryTimeMs: event.target.checked ? (properties.expiryTimeMs ?? Date.now()) : null,
              })
            }
          />
          Expires
        </label>
      </div>
      <Input
        id={id}
        type={type}
        value={localInputValue(properties.expiryTimeMs, type)}
        disabled={pending || !properties.expires}
        onChange={event => {
          const expiryTimeMs = inputTimestamp(event.target.value, type, properties.expiryTimeMs);
          onChange({ expiryTimeMs, expires: expiryTimeMs !== null });
        }}
      />
    </Field>
  );
};
