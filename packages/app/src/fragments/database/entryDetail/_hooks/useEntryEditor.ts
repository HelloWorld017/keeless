import { useRef, useState } from 'react';
import type { EntryPropertiesDraft } from '../_types/EntryPropertiesDraft';
import type { FieldDraft } from '../_types/FieldDraft';
import type { EntryDetailResult, EntryFieldUpdate, EntryPropertiesUpdate } from '@keeless/schema';

export type EntryEditorOptions = {
  fieldSeeds?: Array<{ key: string; name: string; isProtected: boolean }>;
  fieldProtection?: ReadonlyMap<string, boolean>;
};

const createPropertiesDraft = (detail: EntryDetailResult): EntryPropertiesDraft => ({
  overrideUrl: detail.overrideUrl,
  tags: detail.tags.join(', '),
  tagsChanged: false,
  expires: detail.expires,
  expiryTimeMs: detail.expiryTimeMs,
});

const createDrafts = (detail: EntryDetailResult, options: EntryEditorOptions): FieldDraft[] => {
  const drafts: FieldDraft[] = detail.fields.map(field => ({
    key: field.fieldId,
    fieldId: field.fieldId,
    kind: field.kind,
    name: field.name,
    value: field.isProtected ? null : field.value,
    isProtected: options.fieldProtection?.get(field.fieldId) ?? field.isProtected,
    originalIsProtected: field.isProtected,
    valueChanged: false,
  }));
  drafts.push(
    ...(options.fieldSeeds ?? []).map(seed => ({
      ...seed,
      fieldId: null,
      kind: 'custom' as const,
      value: '',
      originalIsProtected: false,
      valueChanged: true,
    })),
  );
  return drafts;
};

const emptyProperties: EntryPropertiesDraft = {
  overrideUrl: '',
  tags: '',
  tagsChanged: false,
  expires: false,
  expiryTimeMs: null,
};

export const useEntryEditor = () => {
  const nextKey = useRef(0);
  const [drafts, setDrafts] = useState<FieldDraft[]>([]);
  const [properties, setProperties] = useState<EntryPropertiesDraft>(emptyProperties);
  const [errors, setErrors] = useState(new Set<string>());

  const begin = (detail: EntryDetailResult, options: EntryEditorOptions = {}) => {
    setDrafts(createDrafts(detail, options));
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
        fieldId: null,
        kind: 'custom',
        name: '',
        value: '',
        isProtected: false,
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
