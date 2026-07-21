import { useRef, useState } from 'react';
import type { EntryPropertiesDraft } from '../_types/EntryPropertiesDraft';
import type { FieldDraft } from '../_types/FieldDraft';
import type { EntryDetailResult, EntryFieldUpdate, EntryPropertiesUpdate } from '@keeless/schema';

const createPropertiesDraft = (detail: EntryDetailResult): EntryPropertiesDraft => ({
  overrideUrl: detail.overrideUrl,
  tags: detail.tags.join(', '),
  tagsChanged: false,
  expires: detail.expires,
  expiryTimeMs: detail.expiryTimeMs,
});

const createDrafts = (detail: EntryDetailResult): FieldDraft[] =>
  detail.fields
    .filter(field => field.type === 'field')
    .map(field => {
      const controlledProtection =
        field.control?.type === 'text' || field.control?.type === 'popout'
          ? field.control.protected
          : undefined;
      return {
        key: field.fieldId ?? `template-${field.order}`,
        order: field.order,
        fieldId: field.fieldId,
        kind: field.kind,
        name: field.name,
        label: field.label,
        control: field.control,
        value: field.fieldId !== null && field.isProtected ? null : field.value,
        isProtected: controlledProtection ?? field.isProtected,
        isInternal: field.isInternal,
        originalIsProtected: field.fieldId !== null && field.isProtected,
        valueChanged: field.fieldId === null,
      };
    });

const emptyProperties: EntryPropertiesDraft = {
  overrideUrl: '',
  tags: '',
  tagsChanged: false,
  expires: false,
  expiryTimeMs: null,
};

export const useEntryEditor = () => {
  const nextKey = useRef(0);
  const nextOrder = useRef(0);
  const [drafts, setDrafts] = useState<FieldDraft[]>([]);
  const [properties, setProperties] = useState<EntryPropertiesDraft>(emptyProperties);
  const [errors, setErrors] = useState(new Set<string>());

  const begin = (detail: EntryDetailResult) => {
    nextOrder.current =
      detail.fields.reduce((maximum, field) => Math.max(maximum, field.order), -1) + 1;
    setDrafts(createDrafts(detail));
    setProperties(createPropertiesDraft(detail));
    setErrors(new Set());
  };

  const clear = () => {
    setDrafts([]);
    setErrors(new Set());
  };

  const add = () =>
    setDrafts(current => [
      ...current,
      {
        key: `new-${nextKey.current++}`,
        order: nextOrder.current++,
        fieldId: null,
        kind: 'custom',
        name: '',
        label: '',
        control: null,
        value: '',
        isProtected: false,
        isInternal: false,
        originalIsProtected: false,
        valueChanged: true,
      },
    ]);

  const load = (key: string, value: string) =>
    setDrafts(current =>
      current.map(field =>
        field.key === key && !field.valueChanged ? { ...field, value } : field,
      ),
    );

  const change = (key: string, patch: Partial<FieldDraft>) => {
    setDrafts(current =>
      current.map(field => (field.key === key ? { ...field, ...patch } : field)),
    );
    if ('name' in patch) {
      setErrors(current => {
        const next = new Set(current);
        next.delete(key);
        return next;
      });
    }
  };

  const remove = (key: string) => setDrafts(current => current.filter(field => field.key !== key));

  const validate = () => {
    const invalid = new Set(
      drafts.filter(field => field.kind === 'custom' && !field.name.trim()).map(field => field.key),
    );
    setErrors(invalid);
    return invalid.size === 0;
  };

  const fieldUpdates = (): EntryFieldUpdate[] =>
    drafts.map(field => ({
      fieldId: field.fieldId,
      name: field.kind === 'custom' ? field.name.trim() : field.name,
      value: field.originalIsProtected && !field.valueChanged ? null : field.value,
      isProtected: field.isProtected,
    }));

  const propertiesUpdate = (detail: EntryDetailResult): EntryPropertiesUpdate => ({
    overrideUrl: properties.overrideUrl,
    tags: properties.tagsChanged
      ? properties.tags
          .split(',')
          .map(tag => tag.trim())
          .filter(Boolean)
      : detail.tags,
    expires: properties.expires,
    expiryTimeMs: properties.expires ? properties.expiryTimeMs : null,
  });

  return {
    drafts,
    properties,
    errors,
    begin,
    clear,
    add,
    load,
    change,
    remove,
    validate,
    fieldUpdates,
    propertiesUpdate,
    changeProperties: (patch: Partial<EntryPropertiesDraft>) =>
      setProperties(current => ({ ...current, ...patch })),
  };
};
