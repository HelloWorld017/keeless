import { Button } from '@/components/button';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/sheet';
import { cn } from '@/utils/css';
import { useState } from 'react';
import { useProtectedReveal } from '../_hooks/useProtectedReveal';
import type { DatabaseNodeId, EntryFieldInformation } from '@keeless/schema';

export const PopoutFieldValue = ({
  entryId,
  field,
  label,
}: {
  entryId: DatabaseNodeId;
  field?: EntryFieldInformation;
  label: string;
}) => {
  const [open, setOpen] = useState(false);
  const [protectedValue, setProtectedValue] = useState<string>();
  const isProtected = field?.isProtected ?? false;
  const { pending, error, revealed, toggleReveal, prompt, promptOpen } = useProtectedReveal({
    entryId,
    fieldId: isProtected ? (field?.fieldId ?? null) : null,
    onReveal: setProtectedValue,
  });
  const value = isProtected ? protectedValue : field?.value;

  return (
    <div className="space-y-1 px-4 py-3">
      <dt className="text-xs text-muted-foreground">{label || 'Untitled field'}</dt>
      <dd>
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-label={`${isProtected && !revealed ? 'Reveal' : 'Open'} ${label || 'field'}`}
          disabled={pending}
          onClick={() => (isProtected && !revealed ? void toggleReveal() : setOpen(true))}
        >
          {isProtected && !revealed ? 'Reveal' : 'Open'}
        </Button>
      </dd>
      {error && !promptOpen && <p className="text-xs text-destructive">{error}</p>}
      {prompt}
      <Sheet
        open={open}
        onOpenChange={nextOpen => {
          setOpen(nextOpen);
          if (!nextOpen && isProtected && revealed) {
            void toggleReveal();
          }
        }}
      >
        <SheetContent className="p-4 sm:max-w-xl sm:p-6">
          <SheetHeader className="p-0">
            <SheetTitle>{label || 'Untitled field'}</SheetTitle>
            <SheetDescription>Expanded field value</SheetDescription>
          </SheetHeader>
          <div className={cn('whitespace-pre-wrap break-words', !value && 'text-muted-foreground')}>
            {value || 'Empty'}
          </div>
        </SheetContent>
      </Sheet>
    </div>
  );
};
