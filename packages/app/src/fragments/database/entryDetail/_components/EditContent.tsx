import { Button } from '@/components/button';
import { Field, FieldError, FieldGroup, FieldLabel } from '@/components/field';
import { Input } from '@/components/input';
import { IconPlus, IconTrash } from '@/icons';
import { ExpiryEditor } from '../_layout/ExpiryEditor';
import { FieldDivider } from '../_layout/FieldDivider';
import { PasswordConfirmationEditor } from '../_layout/PasswordConfirmationEditor';
import { getFieldName } from '../_utils/getFieldName';
import { EntryFieldEditor } from './EntryFieldEditor';
import type { EntryPropertiesDraft } from '../_types/EntryPropertiesDraft';
import type { FieldDraft } from '../_types/FieldDraft';
import type { DatabaseNodeId, EntryFieldInformation } from '@keeless/schema';

type OrderedItem =
  | { type: 'draft'; order: number; draft: FieldDraft }
  | { type: 'template'; order: number; field: Exclude<EntryFieldInformation, { type: 'field' }> };

export const EditContent = ({
  entryId,
  drafts,
  fields,
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
  fields: EntryFieldInformation[];
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
}) => {
  const ordered: OrderedItem[] = [
    ...drafts.map(draft => ({ type: 'draft' as const, order: draft.order, draft })),
    ...fields
      .filter(field => field.type !== 'field')
      .map(field => ({ type: 'template' as const, order: field.order, field })),
  ].toSorted((left, right) => left.order - right.order);

  return (
    <div className="w-full max-w-3xl space-y-8 p-4 sm:p-6">
      <FieldGroup className="gap-5">
        {ordered.map(item => {
          if (item.type === 'template') {
            const { field } = item;
            const id = `template-${field.order}`;
            if (field.type === 'divider') {
              return <FieldDivider key={id} label={field.label} editing />;
            }
            if (field.type === 'passwordConfirmation') {
              return (
                <PasswordConfirmationEditor
                  key={id}
                  id={`${id}-confirmation`}
                  label={field.label}
                  value={confirmations[field.passwordFieldId] ?? ''}
                  invalid={confirmationErrors.has(field.passwordFieldId)}
                  pending={pending}
                  onChange={value => onConfirmationChange(field.passwordFieldId, value)}
                />
              );
            }
            if (field.type === 'overrideUrl' || field.type === 'tags') {
              const tags = field.type === 'tags';
              return (
                <Field key={id}>
                  <FieldLabel htmlFor={id}>{field.label}</FieldLabel>
                  <Input
                    id={id}
                    type={tags ? 'text' : 'url'}
                    value={tags ? properties.tags : properties.overrideUrl}
                    placeholder={tags ? 'Comma-separated tags' : undefined}
                    disabled={pending}
                    onChange={event =>
                      onPropertiesChange(
                        tags
                          ? { tags: event.target.value, tagsChanged: true }
                          : { overrideUrl: event.target.value },
                      )
                    }
                  />
                </Field>
              );
            }
            return (
              <ExpiryEditor
                key={id}
                id={`${id}-expiry`}
                label={field.label}
                control={field.control}
                properties={properties}
                pending={pending}
                onChange={onPropertiesChange}
              />
            );
          }

          const { draft } = item;
          const standard = draft.kind !== 'custom';
          const configured = draft.control !== null;
          const invalid = errors.has(draft.key);
          const nameId = `${draft.key}-name`;
          const valueId = `${draft.key}-value`;
          const errorId = `${draft.key}-error`;
          const editorName = configured
            ? draft.label
            : getFieldName(draft.kind, draft.name) || 'Custom field';
          const editor = (
            <EntryFieldEditor
              entryId={entryId}
              draft={draft}
              label={editorName}
              control={draft.control ?? undefined}
              placeholder={standard || configured ? undefined : 'Enter a value'}
              pending={pending}
              onLoad={value => onLoad(draft.key, value)}
              onChange={value => onChange(draft.key, { value, valueChanged: true })}
            />
          );

          return (
            <div key={draft.key}>
              {standard || configured ? (
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
};
