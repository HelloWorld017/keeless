import { Button } from '@/components/button';
import { Input } from '@/components/input';
import { IconPlus, IconTrash } from '@/icons';
import { DatabaseNodeId } from '@keeless/schema';
import { STANDARD_NAMES } from '../_constants';
import { FieldDraft } from '../_types/FieldDraft';
import { DetailSection } from './DetailSection';
import { FieldPasswordEditor } from './FieldPasswordEditor';
import { FieldPlainEditor } from './FieldPlainEditor';

export const EditContent = ({
  entryId,
  drafts,
  errors,
  pending,
  onAdd,
  onChange,
  onDelete,
}: {
  entryId: DatabaseNodeId;
  drafts: FieldDraft[];
  errors: Set<string>;
  pending: boolean;
  onAdd: () => void;
  onChange: (key: string, patch: Partial<FieldDraft>) => void;
  onDelete: (key: string) => void;
}) => (
  <div className="w-full max-w-3xl space-y-8 p-4 sm:p-6">
    <DetailSection
      title="Fields"
      action={
        <Button type="button" variant="outline" size="sm" disabled={pending} onClick={onAdd}>
          <IconPlus />
          Add field
        </Button>
      }
    >
      <div className="divide-y rounded-lg border">
        {drafts.map(draft => {
          const standard = draft.fieldIndex !== null && draft.fieldIndex < STANDARD_NAMES.length;
          const displayValue = draft.valueChanged
            ? (draft.value ?? '')
            : (draft.revealedValue ?? draft.value ?? '');
          return (
            <div key={draft.key} className="space-y-2 p-3">
              <div className="flex items-start gap-2">
                {standard ? (
                  <span className="min-w-0 flex-1 px-1 py-2 text-xs font-medium text-muted-foreground">
                    {draft.name}
                  </span>
                ) : (
                  <div className="min-w-0 flex-1 space-y-1">
                    <Input
                      value={draft.name}
                      placeholder="Field name"
                      aria-label="Custom field name"
                      aria-invalid={errors.has(draft.key)}
                      aria-describedby={errors.has(draft.key) ? `${draft.key}-error` : undefined}
                      disabled={pending}
                      onChange={event => onChange(draft.key, { name: event.target.value })}
                    />
                    {errors.has(draft.key) && (
                      <p
                        id={`${draft.key}-error`}
                        className="text-xs text-destructive"
                        role="alert"
                      >
                        Enter a field name.
                      </p>
                    )}
                  </div>
                )}
                {!standard && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    className="text-destructive"
                    aria-label={`Delete ${draft.name || 'custom field'}`}
                    disabled={pending}
                    onClick={() => onDelete(draft.key)}
                  >
                    <IconTrash />
                  </Button>
                )}
              </div>
              {draft.isProtected ? (
                <FieldPasswordEditor
                  entryId={entryId}
                  fieldIndex={draft.fieldIndex}
                  name={draft.name || 'Custom field'}
                  value={displayValue}
                  existing={draft.fieldIndex !== null && !draft.valueChanged}
                  disabled={pending}
                  onChange={value => onChange(draft.key, { value, valueChanged: true })}
                  onReveal={revealedValue => onChange(draft.key, { revealedValue })}
                />
              ) : (
                <FieldPlainEditor
                  name={draft.name || 'Custom field'}
                  value={displayValue}
                  multiline={draft.fieldIndex === 4}
                  disabled={pending}
                  onChange={value => onChange(draft.key, { value, valueChanged: true })}
                />
              )}
            </div>
          );
        })}
      </div>
    </DetailSection>
  </div>
);
