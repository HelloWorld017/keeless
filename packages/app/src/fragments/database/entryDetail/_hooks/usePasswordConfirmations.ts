import { useState } from 'react';
import type { FieldDraft } from '../_types/FieldDraft';
import type { EntryFieldInformation } from '@keeless/schema';

export const usePasswordConfirmations = () => {
  const [values, setValues] = useState<Record<string, string>>({});
  const [errors, setErrors] = useState(new Set<string>());

  const clear = () => {
    setValues({});
    setErrors(new Set());
  };

  const change = (fieldId: string, value: string) => {
    setValues(current => ({ ...current, [fieldId]: value }));
    setErrors(current => {
      const next = new Set(current);
      next.delete(fieldId);
      return next;
    });
  };

  const validate = (fields: EntryFieldInformation[], drafts: FieldDraft[]) => {
    const invalid = new Set<string>();
    fields.forEach(field => {
      if (field.type !== 'passwordConfirmation') {
        return;
      }
      const { passwordFieldId } = field;
      const password = drafts.find(draft => draft.fieldId === passwordFieldId);
      if (password?.valueChanged && values[passwordFieldId] !== password.value) {
        invalid.add(passwordFieldId);
      }
    });
    setErrors(invalid);
    return invalid.size === 0;
  };

  return { values, errors, clear, change, validate };
};
