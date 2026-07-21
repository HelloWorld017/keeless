import { Field, FieldLabel } from '@/components/field';
import { Input } from '@/components/input';
import { EntryFieldEditor } from '../_components/EntryFieldEditor';
import { ExpiryEditor } from './ExpiryEditor';
import { FieldDivider } from './FieldDivider';
import { PasswordConfirmationEditor } from './PasswordConfirmationEditor';
import { getLayoutItemKey, resolveLayoutField } from './bindings';
import type { EntryPropertiesDraft } from '../_types/EntryPropertiesDraft';
import type { FieldDraft } from '../_types/FieldDraft';
import type { DatabaseNodeId, EntryLayout } from '@keeless/schema';

export const EntryLayoutEditor = ({
  entryId,
  layout,
  drafts,
  properties,
  confirmations,
  confirmationErrors,
  pending,
  onLoad,
  onChange,
  onPropertiesChange,
  onConfirmationChange,
}: {
  entryId: DatabaseNodeId;
  layout: EntryLayout;
  drafts: FieldDraft[];
  properties: EntryPropertiesDraft;
  confirmations: Record<string, string>;
  confirmationErrors: Set<string>;
  pending: boolean;
  onLoad: (key: string, value: string) => void;
  onChange: (key: string, patch: Partial<FieldDraft>) => void;
  onPropertiesChange: (patch: Partial<EntryPropertiesDraft>) => void;
  onConfirmationChange: (fieldId: string, value: string) => void;
}) => (
  <>
    {layout.items.map((item, index) => {
      const { target, control, label } = item;
      const key = getLayoutItemKey(layout.items, index);
      const id = `layout-${index}`;
      if (target.type === 'divider' || control.type === 'divider') {
        return <FieldDivider key={key} label={label} editing />;
      }
      if (target.type === 'passwordConfirmation') {
        return (
          <PasswordConfirmationEditor
            key={key}
            id={`${id}-confirmation`}
            label={label}
            value={confirmations[target.passwordFieldId] ?? ''}
            invalid={confirmationErrors.has(target.passwordFieldId)}
            pending={pending}
            onChange={value => onConfirmationChange(target.passwordFieldId, value)}
          />
        );
      }
      if (target.type === 'overrideUrl' || target.type === 'tags') {
        const tags = target.type === 'tags';
        return (
          <Field key={key}>
            <FieldLabel htmlFor={id}>{label}</FieldLabel>
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
      if (target.type === 'expiry') {
        return (
          <ExpiryEditor
            key={key}
            id={`${id}-expiry`}
            label={label}
            control={control}
            properties={properties}
            pending={pending}
            onChange={onPropertiesChange}
          />
        );
      }
      if (target.type !== 'field') {
        return null;
      }
      const draft = resolveLayoutField(target, drafts);
      if (!draft) {
        return null;
      }
      return (
        <Field key={key}>
          <FieldLabel htmlFor={`${id}-value`}>{label}</FieldLabel>
          <EntryFieldEditor
            entryId={entryId}
            draft={draft}
            id={`${id}-value`}
            label={label}
            control={control}
            pending={pending}
            onLoad={value => onLoad(draft.key, value)}
            onChange={value => onChange(draft.key, { value, valueChanged: true })}
          />
        </Field>
      );
    })}
  </>
);
