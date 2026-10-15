import { useEffect, useRef, useState } from 'react';

/** Owns one password prompt and settles its promise on submit, cancel, or unmount. */
export const usePasswordRequest = <T>() => {
  const mounted = useRef(true);
  const resolver = useRef<((password: string | null) => void) | undefined>(undefined);
  const [request, setRequest] = useState<T>();

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      resolver.current?.(null);
      resolver.current = undefined;
    };
  }, []);

  const settle = (password: string | null) => {
    resolver.current?.(password);
    resolver.current = undefined;
    if (mounted.current) {
      setRequest(undefined);
    }
  };

  const ask = (options: T): Promise<string | null> => {
    if (!mounted.current) {
      return Promise.resolve(null);
    }
    resolver.current?.(null);
    return new Promise(resolve => {
      resolver.current = resolve;
      setRequest(options);
    });
  };

  return { request, ask, submit: settle, cancel: () => settle(null) };
};
