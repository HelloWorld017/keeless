import { Button } from '@/components/button';
import { Field, FieldError, FieldGroup, FieldLabel } from '@/components/field';
import { Input } from '@/components/input';
import { IconPlus, IconTrash } from '@/icons';
import { STANDARD_NAMES } from '../_constants';
import { FieldPasswordEditor } from './FieldPasswordEditor';
import { FieldPlainEditor } from './FieldPlainEditor';
import type { FieldDraft } from '../_types/FieldDraft';
import type { DatabaseNodeId } from '@keeless/schema';
import {getFieldName} from '../_utils/getFieldName';

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
    <FieldGroup className="gap-5">
      {drafts.map(draft => {
        const standard = draft.fieldIndex !== null && draft.fieldIndex < STANDARD_NAMES.length;
        const invalid = errors.has(draft.key);
        const nameId = `${draft.key}-name`;
        const valueId = `${draft.key}-value`;
        const errorId = `${draft.key}-error`;
        const displayValue = draft.value ?? '';
        const editorName = draft.name ? getFieldName(draft.name) : 'Custom field';

        const editor = draft.isProtected ? (
          <FieldPasswordEditor
            id={valueId}
            entryId={entryId}
            fieldIndex={draft.fieldIndex}
            name={editorName}
            value={displayValue}
            existing={draft.fieldIndex !== null && !draft.valueChanged}
            disabled={pending}
            onChange={value => onChange(draft.key, { value, valueChanged: true })}
          />
        ) : (
          <FieldPlainEditor
            id={valueId}
            name={editorName}
            value={displayValue}
            placeholder={standard ? undefined : 'Enter a value'}
            multiline={draft.fieldIndex === STANDARD_NAMES.indexOf('Notes')}
            disabled={pending}
            onChange={value => onChange(draft.key, { value, valueChanged: true })}
          />
        );
        return (
          <div key={draft.key}>
            {standard ? (
              <Field>
                <FieldLabel htmlFor={valueId}>{editorName}</FieldLabel>
                {editor}
              </Field>
            ) : (
              <FieldGroup className="gap-3">
                <FieldLabel htmlFor={valueId}>{editorName}</FieldLabel>
                <div className="flex items-center gap-2">
                  <Field data-invalid={invalid} className="flex-1 min-w-0 gap-1.5">
                    <Input
                      id={nameId}
                      value={draft.name}
                      placeholder="e.g. Recovery email"
                      aria-invalid={invalid}
                      aria-describedby={invalid ? errorId : undefined}
                      disabled={pending}
                      onChange={event => onChange(draft.key, { name: event.target.value })}
                    />
                    {invalid && (
                      <FieldError id={errorId} className="text-xs">
                        Enter a field name.
                      </FieldError>
                    )}
                  </Field>
                  <Field className="flex-2">
                    {editor}
                  </Field>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    className="text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                    aria-label={`Delete ${draft.name || 'custom field'}`}
                    disabled={pending}
                    onClick={() => onDelete(draft.key)}
                  >
                    <IconTrash />
                  </Button>
                </div>
              </FieldGroup>
            )}
          </div>
        );
      })}
    </FieldGroup>
    <Button
      type="button"
      variant="outline"
      className="w-full border-dashed text-muted-foreground hover:text-foreground"
      disabled={pending}
      onClick={onAdd}
    >
      <IconPlus />
      Add field
    </Button>
  </div>
);
