import { Button } from '@/components/button';
import { IconLoaderCircle } from '@/icons';
import { cx } from '@/utils/css';
import { useProtectedReveal } from '../_hooks/useProtectedReveal';
import type { DatabaseNodeId } from '@keeless/schema';

export const FieldNoteEditor = ({
  id,
  entryId,
  fieldId,
  name,
  value,
  isProtected,
  existing,
  lines = 4,
  disabled,
  onLoad,
  onChange,
}: {
  id?: string;
  entryId: DatabaseNodeId;
  fieldId: string | null;
  name: string;
  value: string;
  isProtected: boolean;
  existing: boolean;
  lines?: number;
  disabled: boolean;
  onLoad: (value: string) => void;
  onChange: (value: string) => void;
}) => {
  const { pending, error, revealed, toggleReveal, prompt, promptOpen } = useProtectedReveal({
    entryId,
    fieldId: isProtected ? fieldId : null,
    autoReveal: isProtected && existing,
    onReveal: nextValue => {
      if (existing && nextValue !== undefined) {
        onLoad(nextValue);
      }
    },
  });
  const unloaded = isProtected && existing && !revealed;

  return (
    <>
      <div className="flex items-start gap-2">
        <textarea
          id={id}
          value={value}
          rows={Math.max(1, lines)}
          className={cx(
            'min-h-8 w-full min-w-0 resize-y rounded-lg border border-input bg-transparent px-2.5 py-2 text-base outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:bg-input/50 disabled:opacity-50 md:text-sm dark:bg-input/30 dark:disabled:bg-input/80',
          )}
          placeholder={unloaded ? '(unchanged)' : undefined}
          aria-label={`${name} value`}
          disabled={disabled}
          onChange={event => onChange(event.target.value)}
        />
        {unloaded && (
          <Button
            type="button"
            variant="outline"
            disabled={disabled || pending}
            onClick={() => void toggleReveal()}
          >
            {pending && <IconLoaderCircle className="animate-spin" />}
            Load
          </Button>
        )}
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
