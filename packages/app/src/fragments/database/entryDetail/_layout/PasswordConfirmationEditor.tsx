import { Field, FieldError, FieldLabel } from '@/components/field';
import { Input } from '@/components/input';

export const PasswordConfirmationEditor = ({
  id,
  label,
  value,
  invalid,
  pending,
  onChange,
}: {
  id: string;
  label: string;
  value: string;
  invalid: boolean;
  pending: boolean;
  onChange: (value: string) => void;
}) => (
  <Field data-invalid={invalid}>
    <FieldLabel htmlFor={id}>{label}</FieldLabel>
    <Input
      id={id}
      type="password"
      value={value}
      aria-invalid={invalid}
      disabled={pending}
      onChange={event => onChange(event.target.value)}
    />
    {invalid && <FieldError>Password confirmation does not match.</FieldError>}
  </Field>
);
