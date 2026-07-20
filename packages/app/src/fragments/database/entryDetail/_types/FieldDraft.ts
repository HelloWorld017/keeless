export type FieldDraft = {
  key: string;
  fieldIndex: number | null;
  name: string;
  value: string | null;
  isProtected: boolean;
  valueChanged: boolean;
  revealedValue?: string;
};
