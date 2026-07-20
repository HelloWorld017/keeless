import { Input } from '@/components/input';
import type { DatabaseNodeId } from '@keeless/schema';
import {useProtectedReveal} from '../_hooks/useProtectedReveal';
import {Button} from '@/components/button';
import {IconEye, IconEyeOff, IconLoaderCircle} from '@/icons';

export const FieldPasswordEditor = ({
  id,
  entryId,
  fieldIndex,
  name,
  value,
  existing,
  disabled,
  onChange,
}: {
  id?: string;
  entryId: DatabaseNodeId;
  fieldIndex: number | null;
  name: string;
  value: string;
  existing: boolean;
  disabled: boolean;
  onChange: (value: string) => void;
}) => {
  const { pending, error, revealed, toggleReveal, prompt, promptOpen } = useProtectedReveal({
    entryId,
    fieldIndex,
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
          variant='outline'
          size='icon'
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
