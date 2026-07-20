import { buildContext } from '@/utils/context';
import { useCallback, useEffect, useRef, useState } from 'react';

export type ToastKind = 'default' | 'primary' | 'destructive';

export type ToastItem = {
  id: number;
  message: string;
  kind: ToastKind;
};

export type ShowToastInput = {
  message: string;
  kind?: ToastKind;
  durationMs?: number;
};

const DEFAULT_DURATION_MS = 10000;

const [ToastProvider, useToast] = buildContext(() => {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const nextToastIdRef = useRef(0);
  const autoDismissTimeoutMapRef = useRef(new Map<number, number>());

  const clearAutoDismissTimeout = useCallback((id: number) => {
    const timeoutId = autoDismissTimeoutMapRef.current.get(id);
    if (timeoutId === undefined) {
      return;
    }

    window.clearTimeout(timeoutId);
    autoDismissTimeoutMapRef.current.delete(id);
  }, []);

  const dismissToast = useCallback(
    (id: number) => {
      clearAutoDismissTimeout(id);
      setToasts(currentToasts => currentToasts.filter(toast => toast.id !== id));
    },
    [clearAutoDismissTimeout],
  );

  const dismissAllToasts = useCallback(() => {
    autoDismissTimeoutMapRef.current.forEach(timeoutId => {
      window.clearTimeout(timeoutId);
    });
    autoDismissTimeoutMapRef.current.clear();
    setToasts([]);
  }, []);

  const showToast = useCallback(
    ({ durationMs = DEFAULT_DURATION_MS, kind = 'default', message }: ShowToastInput) => {
      const id = nextToastIdRef.current + 1;
      nextToastIdRef.current = id;

      setToasts(currentToasts => [...currentToasts, { id, message, kind }]);

      if (durationMs > 0) {
        const timeoutId = window.setTimeout(() => {
          dismissToast(id);
        }, durationMs);
        autoDismissTimeoutMapRef.current.set(id, timeoutId);
      }

      return id;
    },
    [dismissToast],
  );

  useEffect(
    () => () => {
      autoDismissTimeoutMapRef.current.forEach(timeoutId => {
        window.clearTimeout(timeoutId);
      });
      autoDismissTimeoutMapRef.current.clear();
    },
    [],
  );

  return {
    toasts,
    showToast,
    dismissToast,
    dismissAllToasts,
  };
});

export { ToastProvider };
export const useToasts = () => useToast(state => state.toasts);
export const useShowToast = () => useToast(state => state.showToast);
export const useDismissToast = () => useToast(state => state.dismissToast);
export const useDismissAllToasts = () => useToast(state => state.dismissAllToasts);
