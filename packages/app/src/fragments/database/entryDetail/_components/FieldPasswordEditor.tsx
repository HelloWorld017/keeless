import { Input } from '@/components/input';
import { FieldPassword } from './FieldPassword';
import type { DatabaseNodeId } from '@keeless/schema';

export const FieldPasswordEditor = ({
  entryId,
  fieldIndex,
  name,
  value,
  existing,
  disabled,
  onChange,
  onReveal,
}: {
  entryId: DatabaseNodeId;
  fieldIndex: number | null;
  name: string;
  value: string;
  existing: boolean;
  disabled: boolean;
  onChange: (value: string) => void;
  onReveal: (value: string) => void;
}) => (
  <div className="space-y-2">
    <Input
      type="password"
      value={value}
      placeholder={existing ? 'Protected value unchanged' : undefined}
      autoComplete="new-password"
      aria-label={`${name} value`}
      disabled={disabled}
      onChange={event => onChange(event.target.value)}
    />
    {existing && fieldIndex !== null && !value && (
      <FieldPassword
        entryId={entryId}
        fieldIndex={fieldIndex}
        name={name}
        onReveal={onReveal}
        compact
      />
    )}
  </div>
);
