import { Input } from '@/components/input';

export const FieldPlainEditor = ({
  id,
  name,
  value,
  placeholder,
  disabled,
  onChange,
}: {
  id?: string;
  name: string;
  value: string;
  placeholder?: string;
  disabled: boolean;
  onChange: (value: string) => void;
}) => (
  <Input
    id={id}
    value={value}
    placeholder={placeholder}
    aria-label={`${name} value`}
    disabled={disabled}
    onChange={event => onChange(event.target.value)}
  />
);
