import { useLayoutEffect, useRef } from 'react';

export const useLatestRef = <T>(value: T) => {
  const valueRef = useRef(value);
  useLayoutEffect(() => {
    valueRef.current = value;
  }, [value]);

  return valueRef;
};
