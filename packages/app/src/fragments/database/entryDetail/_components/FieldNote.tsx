import { Button } from '@/components/button';
import { IconLoaderCircle } from '@/icons';
import { cn } from '@/utils/css';
import { useState } from 'react';
import { useProtectedReveal } from '../_hooks/useProtectedReveal';
import type { DatabaseNodeId } from '@keeless/schema';

export const FieldNote = ({
  entryId,
  fieldId,
  name,
  value,
  isProtected,
}: {
  entryId: DatabaseNodeId;
  fieldId: string;
  name: string;
  value: string | null;
  isProtected: boolean;
}) => {
  const [protectedValue, setProtectedValue] = useState<string>();
  const { pending, error, revealed, toggleReveal, prompt, promptOpen } = useProtectedReveal({
    entryId,
    fieldId: isProtected ? fieldId : null,
    onReveal: setProtectedValue,
  });
  const displayValue = isProtected ? protectedValue : value;

  return (
    <div className="space-y-1 px-4 py-3">
      <dt className="text-xs text-muted-foreground">{name || 'Untitled field'}</dt>
      <dd className="flex min-w-0 items-start gap-2 text-sm">
        <span
          className={cn(
            'min-w-0 flex-1 whitespace-pre-wrap break-words',
            (!isProtected || revealed) && !displayValue && 'text-muted-foreground',
            isProtected && !revealed && 'text-muted-foreground',
          )}
        >
          {isProtected && !revealed ? '(protected)' : displayValue || 'Empty'}
        </span>
        {isProtected && !revealed && (
          <Button
            type="button"
            variant="outline"
            size="xs"
            disabled={pending}
            onClick={() => void toggleReveal()}
          >
            {pending && <IconLoaderCircle className="animate-spin" />}
            View
          </Button>
        )}
      </dd>
      {error && !promptOpen && (
        <p className="text-xs text-destructive" role="alert">
          {error}
        </p>
      )}
      {prompt}
    </div>
  );
};
