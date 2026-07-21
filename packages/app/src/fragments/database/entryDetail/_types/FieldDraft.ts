import type { EntryFieldKind } from '@keeless/schema';

export type FieldDraft = {
  key: string;
  fieldId: string | null;
  kind: EntryFieldKind;
  name: string;
  value: string | null;
  isProtected: boolean;
  originalIsProtected: boolean;
  valueChanged: boolean;
};
