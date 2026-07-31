import { Button } from '@/components/button';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/sheet';
import { cx } from '@/utils/css';
import { useState } from 'react';
import { useEntryFieldValueActions } from '../_hooks/useEntryFieldValues';
import { FieldCopyButton } from './FieldCopyButton';
import type { EntryFieldInformation } from '@keeless/schema';

type EntryField = Extract<EntryFieldInformation, { type: 'field' }>;

export const PopoutFieldValue = ({ field, label }: { field: EntryField; label: string }) => {
  const [open, setOpen] = useState(false);
  const isProtected = field.fieldId !== null && field.isProtected;
  const actions = useEntryFieldValueActions();
  const protectedValue = field.fieldId === null ? undefined : actions.values[field.fieldId];
  const pending = field.fieldId === null ? false : actions.pendingFieldIds.has(field.fieldId);
  const revealed = protectedValue !== undefined;
  const value = isProtected ? protectedValue : field.value;

  return (
    <div className="space-y-1 px-4 py-3">
      <dt className="text-xs text-muted-foreground">{label || 'Untitled field'}</dt>
      <dd className="flex items-center gap-2">
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-label={`${isProtected && !revealed ? 'Reveal' : 'Open'} ${label || 'field'}`}
          disabled={pending}
          onClick={() =>
            isProtected && !revealed && field.fieldId !== null
              ? actions.toggleReveal(field.fieldId)
              : setOpen(true)
          }
        >
          {isProtected && !revealed ? 'Reveal' : 'Open'}
        </Button>
        <FieldCopyButton
          label={label}
          value={isProtected ? undefined : field.value}
          fieldId={isProtected && field.fieldId !== null ? field.fieldId : undefined}
        />
      </dd>
      <Sheet
        open={open}
        onOpenChange={nextOpen => {
          setOpen(nextOpen);
          if (!nextOpen && isProtected && revealed && field.fieldId !== null) {
            actions.toggleReveal(field.fieldId);
          }
        }}
      >
        <SheetContent className="p-4 sm:max-w-xl sm:p-6">
          <SheetHeader className="p-0">
            <SheetTitle>{label || 'Untitled field'}</SheetTitle>
            <SheetDescription>Field contents</SheetDescription>
          </SheetHeader>
          <div className={cx('whitespace-pre-wrap break-words', !value && 'text-muted-foreground')}>
            {value || 'Empty'}
          </div>
        </SheetContent>
      </Sheet>
    </div>
  );
};
