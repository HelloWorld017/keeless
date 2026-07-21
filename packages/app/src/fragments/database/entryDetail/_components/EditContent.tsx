import { Button } from '@/components/button';
import { Field, FieldError, FieldGroup, FieldLabel } from '@/components/field';
import { Input } from '@/components/input';
import { IconPlus, IconTrash } from '@/icons';
import { getFieldName } from '../_utils/getFieldName';
import { FieldEtmEditor } from './FieldEtmEditor';
import { FieldNoteEditor } from './FieldNoteEditor';
import { FieldPasswordEditor } from './FieldPasswordEditor';
import { FieldPlainEditor } from './FieldPlainEditor';
import type { EntryPropertiesDraft } from '../_types/EntryPropertiesDraft';
import type { FieldDraft } from '../_types/FieldDraft';
import type { DatabaseNodeId, EtmLayout } from '@keeless/schema';

export const EditContent = ({
  entryId,
  drafts,
  layout,
  properties,
  confirmations,
  confirmationErrors,
  errors,
  pending,
  onAdd,
  onLoad,
  onChange,
  onDelete,
  onPropertiesChange,
  onConfirmationChange,
}: {
  entryId: DatabaseNodeId;
  drafts: FieldDraft[];
  layout: EtmLayout | null;
  properties: EntryPropertiesDraft;
  confirmations: Record<string, string>;
  confirmationErrors: Set<string>;
  errors: Set<string>;
  pending: boolean;
  onAdd: () => void;
  onLoad: (key: string, value: string) => void;
  onChange: (key: string, patch: Partial<FieldDraft>) => void;
  onDelete: (key: string) => void;
  onPropertiesChange: (patch: Partial<EntryPropertiesDraft>) => void;
  onConfirmationChange: (fieldId: string, value: string) => void;
}) => (
  <div className="w-full max-w-3xl space-y-8 p-4 sm:p-6">
    <FieldGroup className="gap-5">
      {layout?.items.map((item, index) => {
        const target = item.target;
        const draft =
          target.type === 'field'
            ? drafts.find(candidate =>
                target.fieldId
                  ? candidate.fieldId === target.fieldId
                  : candidate.fieldId === null && candidate.name === target.fieldName,
              )
            : undefined;
        const confirmationId = target.type === 'passwordConfirmation' ? target.passwordFieldId : '';
        return (
          <FieldEtmEditor
            key={`${item.target.type}:${index}`}
            item={item}
            entryId={entryId}
            draft={draft}
            properties={properties}
            confirmation={confirmations[confirmationId] ?? ''}
            confirmationInvalid={confirmationErrors.has(confirmationId)}
            pending={pending}
            onLoad={value => draft && onLoad(draft.key, value)}
            onChange={patch => draft && onChange(draft.key, patch)}
            onPropertiesChange={onPropertiesChange}
            onConfirmationChange={value => onConfirmationChange(confirmationId, value)}
          />
        );
      })}
      {drafts.map(draft => {
        const referenced = layout?.items.some(
          item =>
            item.target.type === 'field' &&
            (item.target.fieldId
              ? draft.fieldId === item.target.fieldId
              : draft.fieldId === null && draft.name === item.target.fieldName),
        );
        if (referenced) {
          return null;
        }
        const standard = draft.kind !== 'custom';
        const invalid = errors.has(draft.key);
        const nameId = `${draft.key}-name`;
        const valueId = `${draft.key}-value`;
        const errorId = `${draft.key}-error`;
        const displayValue = draft.value ?? '';
        const editorName = getFieldName(draft.kind, draft.name) || 'Custom field';

        const editor =
          draft.kind === 'notes' ? (
            <FieldNoteEditor
              id={valueId}
              entryId={entryId}
              fieldId={draft.fieldId}
              name={editorName}
              value={displayValue}
              isProtected={draft.isProtected}
              existing={draft.fieldId !== null && !draft.valueChanged}
              disabled={pending}
              onLoad={value => onLoad(draft.key, value)}
              onChange={value => onChange(draft.key, { value, valueChanged: true })}
            />
          ) : draft.isProtected ? (
            <FieldPasswordEditor
              id={valueId}
              entryId={entryId}
              fieldId={draft.fieldId}
              name={editorName}
              value={displayValue}
              existing={draft.fieldId !== null && !draft.valueChanged}
              disabled={pending}
              onLoad={value => onLoad(draft.key, value)}
              onChange={value => onChange(draft.key, { value, valueChanged: true })}
            />
          ) : (
            <FieldPlainEditor
              id={valueId}
              name={editorName}
              value={displayValue}
              placeholder={standard ? undefined : 'Enter a value'}
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
                  <Field className="flex-2">{editor}</Field>
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
