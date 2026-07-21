import type { EntryFieldKind, FieldControl } from '@keeless/schema';

export type FieldDraft = {
  key: string;
  order: number;
  fieldId: string | null;
  kind: EntryFieldKind;
  name: string;
  label: string;
  control: FieldControl | null;
  value: string | null;
  isProtected: boolean;
  originalIsProtected: boolean;
  valueChanged: boolean;
};
