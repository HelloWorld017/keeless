import { Button } from '@/components/button';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/sheet';
import { useState } from 'react';
import { FieldNoteEditor } from './FieldNoteEditor';
import type { FieldDraft } from '../_types/FieldDraft';
import type { DatabaseNodeId } from '@keeless/schema';

export const PopoutFieldEditor = ({
  entryId,
  draft,
  id,
  label,
  pending,
  onLoad,
  onChange,
}: {
  entryId: DatabaseNodeId;
  draft: FieldDraft;
  id: string;
  label: string;
  pending: boolean;
  onLoad: (value: string) => void;
  onChange: (value: string) => void;
}) => {
  const [open, setOpen] = useState(false);
  const existing = draft.fieldId !== null && draft.originalIsProtected && !draft.valueChanged;

  return (
    <>
      <Button
        id={id}
        type="button"
        variant="outline"
        disabled={pending}
        onClick={() => setOpen(true)}
      >
        Open
      </Button>
      <Sheet open={open} onOpenChange={setOpen}>
        <SheetContent className="p-4 sm:max-w-xl sm:p-6">
          <SheetHeader className="p-0">
            <SheetTitle>{label}</SheetTitle>
            <SheetDescription>Edit the expanded field value.</SheetDescription>
          </SheetHeader>
          <FieldNoteEditor
            entryId={entryId}
            fieldId={draft.fieldId}
            name={label}
            value={draft.value ?? ''}
            isProtected={draft.originalIsProtected || draft.isProtected}
            existing={existing}
            disabled={pending}
            onLoad={onLoad}
            onChange={onChange}
          />
        </SheetContent>
      </Sheet>
    </>
  );
};
