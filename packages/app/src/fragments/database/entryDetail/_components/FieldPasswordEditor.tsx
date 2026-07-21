import { Button } from '@/components/button';
import { Input } from '@/components/input';
import { IconEye, IconEyeOff, IconLoaderCircle } from '@/icons';
import { useProtectedReveal } from '../_hooks/useProtectedReveal';
import type { DatabaseNodeId } from '@keeless/schema';

export const FieldPasswordEditor = ({
  id,
  entryId,
  fieldId,
  name,
  value,
  existing,
  disabled,
  onChange,
}: {
  id?: string;
  entryId: DatabaseNodeId;
  fieldId: string | null;
  name: string;
  value: string;
  existing: boolean;
  disabled: boolean;
  onChange: (value: string) => void;
}) => {
  const { pending, error, revealed, toggleReveal, prompt, promptOpen } = useProtectedReveal({
    entryId,
    fieldId,
    onReveal: nextValue => {
      if (existing && !value && nextValue) {
        onChange(nextValue);
      }
    },
  });

  return (
    <>
      <div className="flex items-start gap-2">
        <Input
          id={id}
          type={existing || revealed ? 'text' : 'password'}
          value={value}
          placeholder={existing ? '(unchanged)' : undefined}
          autoComplete="new-password"
          aria-label={`${name} value`}
          disabled={disabled}
          onChange={event => onChange(event.target.value)}
        />
        <Button
          type="button"
          variant="outline"
          size="icon"
          aria-label={revealed ? `Hide ${name}` : `Reveal ${name}`}
          aria-pressed={revealed}
          disabled={disabled || pending}
          onClick={toggleReveal}
        >
          {pending ? (
            <IconLoaderCircle className="animate-spin" />
          ) : revealed ? (
            <IconEyeOff />
          ) : (
            <IconEye />
          )}
        </Button>
      </div>
      {prompt}
      {error && !promptOpen && (
        <p className="text-xs text-destructive" role="alert">
          {error}
        </p>
      )}
    </>
  );
};
