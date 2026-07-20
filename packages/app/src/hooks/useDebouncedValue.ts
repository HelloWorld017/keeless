import { debounce } from 'es-toolkit';
import { useEffect, useState } from 'react';

export const useDebouncedValue = <T>(value: T, delayMs: number, initialValue: T = value) => {
  const [debouncedValue, setDebouncedValue] = useState(initialValue);

  useEffect(() => {
    const updateValue = debounce(() => setDebouncedValue(value), delayMs);
    updateValue();

    return updateValue.cancel;
  }, [delayMs, value]);

  return debouncedValue;
};
