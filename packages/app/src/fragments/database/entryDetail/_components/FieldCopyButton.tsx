import { Button } from '@/components/button';
import { IconCopy, IconLoaderCircle } from '@/icons';
import { useEntryFieldValueActions } from '../_hooks/useEntryFieldValues';

export const FieldCopyButton = ({
  label,
  value,
  fieldId,
}: {
  label: string;
  value?: string | null;
  fieldId?: string;
}) => {
  const actions = useEntryFieldValueActions();
  const pending = fieldId ? actions.pendingFieldIds.has(fieldId) : false;
  return (
    <Button
      type="button"
      variant="ghost"
      size="icon-sm"
      className="-my-1 shrink-0 text-muted-foreground"
      aria-label={`Copy ${label || 'field'}`}
      disabled={pending}
      onClick={() =>
        fieldId ? actions.copyProtected(fieldId, label) : actions.copyPublic(value ?? '', label)
      }
    >
      {pending ? <IconLoaderCircle className="animate-spin" /> : <IconCopy />}
    </Button>
  );
};
