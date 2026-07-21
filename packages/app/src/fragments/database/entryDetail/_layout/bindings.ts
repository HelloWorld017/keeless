import type { EntryEditorOptions } from '../_hooks/useEntryEditor';
import type {
  EntryFieldInformation,
  EntryLayout,
  EntryLayoutItem,
  LayoutTarget,
} from '@keeless/schema';

type FieldBinding = {
  fieldId: string | null;
  name: string;
};

export const resolveLayoutField = <T extends FieldBinding>(
  target: LayoutTarget,
  fields: readonly T[],
) => {
  if (target.type !== 'field') {
    return undefined;
  }
  if (target.fieldId !== null) {
    return fields.find(field => field.fieldId === target.fieldId);
  }

  const matches = fields.filter(field => field.name === target.fieldName);
  return matches.length === 1 ? matches[0] : undefined;
};

export const isAmbiguousLayoutField = (target: LayoutTarget, fields: readonly FieldBinding[]) =>
  target.type === 'field' &&
  target.fieldId === null &&
  fields.filter(field => field.name === target.fieldName).length > 1;

export const getLayoutFieldKeys = (
  layout: EntryLayout | null,
  fields: readonly (FieldBinding & { key: string })[],
) =>
  new Set(
    layout?.items.flatMap(item => {
      const field = resolveLayoutField(item.target, fields);
      return field ? [field.key] : [];
    }) ?? [],
  );

const itemSignature = (item: EntryLayoutItem) => JSON.stringify(item);

export const getLayoutItemKey = (items: EntryLayoutItem[], index: number) => {
  const signature = itemSignature(items[index]);
  const occurrence = items.slice(0, index).filter(item => itemSignature(item) === signature).length;
  return `${signature}:${occurrence}`;
};

export const getLayoutEditorOptions = (
  layout: EntryLayout | null,
  fields: EntryFieldInformation[],
): EntryEditorOptions => {
  const fieldSeeds: NonNullable<EntryEditorOptions['fieldSeeds']> = [];
  const fieldProtection = new Map<string, boolean>();

  layout?.items.forEach((item, index) => {
    if (item.target.type !== 'field') {
      return;
    }
    const target = item.target;
    const field = resolveLayoutField(target, fields);
    const isProtected =
      (item.control.type === 'text' || item.control.type === 'popout') && item.control.protected;
    if (field) {
      fieldProtection.set(field.fieldId, isProtected);
      return;
    }

    const nameMatches = fields.filter(candidate => candidate.name === target.fieldName).length;
    if (target.fieldId !== null || nameMatches === 0) {
      fieldSeeds.push({
        key: `layout-${index}`,
        name: target.fieldName,
        isProtected,
      });
    }
  });

  return { fieldSeeds, fieldProtection };
};
