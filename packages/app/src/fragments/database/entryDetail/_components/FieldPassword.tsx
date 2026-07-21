import { Button } from '@/components/button';
import { IconEye, IconEyeOff, IconLoaderCircle } from '@/icons';
import { useEntryFieldValueActions } from '../_hooks/useEntryFieldValues';
import { FieldCopyButton } from './FieldCopyButton';

export const FieldPassword = ({
  fieldId,
  name,
  disabled = false,
}: {
  fieldId: string;
  name: string;
  disabled?: boolean;
}) => {
  const actions = useEntryFieldValueActions();
  const value = actions.values[fieldId];
  const revealed = value !== undefined;
  const pending = actions.pendingFieldIds.has(fieldId);

  const revealButton = (
    <Button
      type="button"
      variant="ghost"
      size="icon-sm"
      className="-my-1 shrink-0 text-muted-foreground"
      aria-label={revealed ? `Hide ${name}` : `Reveal ${name}`}
      aria-pressed={revealed}
      disabled={disabled || pending}
      onClick={() => actions.toggleReveal(fieldId)}
    >
      {pending ? (
        <IconLoaderCircle className="animate-spin" />
      ) : revealed ? (
        <IconEyeOff />
      ) : (
        <IconEye />
      )}
    </Button>
  );

  return (
    <div className="space-y-1 px-4 py-3">
      <dt className="text-xs text-muted-foreground">{name || 'Untitled field'}</dt>
      <dd className="flex min-w-0 items-start gap-2 text-sm">
        <span
          className={`min-w-0 flex-1 break-words ${revealed && !value ? 'text-muted-foreground' : ''}`}
        >
          {revealed ? (
            value || 'Empty'
          ) : (
            <>
              <span className="tracking-[0.2em]" aria-hidden="true">
                ●●●●●●●●●●●
              </span>
              <span className="sr-only">Protected value</span>
            </>
          )}
        </span>
        {revealButton}
        <FieldCopyButton label={name} fieldId={fieldId} />
      </dd>
    </div>
  );
};
