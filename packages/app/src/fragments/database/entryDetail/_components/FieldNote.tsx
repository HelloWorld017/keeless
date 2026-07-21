import { Button } from '@/components/button';
import { IconEye, IconEyeOff, IconLoaderCircle } from '@/icons';
import { cn } from '@/utils/css';
import { useEntryFieldValueActions } from '../_hooks/useEntryFieldValues';
import { FieldCopyButton } from './FieldCopyButton';

export const FieldNote = ({
  fieldId,
  name,
  value,
  isProtected,
}: {
  fieldId: string;
  name: string;
  value: string | null;
  isProtected: boolean;
}) => {
  const actions = useEntryFieldValueActions();
  const protectedValue = actions.values[fieldId];
  const pending = actions.pendingFieldIds.has(fieldId);
  const revealed = protectedValue !== undefined;
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
        {isProtected && (
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="-my-1 shrink-0 text-muted-foreground"
            aria-label={revealed ? `Hide ${name}` : `Reveal ${name}`}
            aria-pressed={revealed}
            disabled={pending}
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
        )}
        <FieldCopyButton
          label={name}
          value={isProtected ? undefined : value}
          fieldId={isProtected ? fieldId : undefined}
        />
      </dd>
    </div>
  );
};
