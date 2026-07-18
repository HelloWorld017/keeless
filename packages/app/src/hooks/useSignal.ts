import { useLayoutEffect, useMemo, useState, useSyncExternalStore } from 'react';
import { useLatestRef } from './useLatestRef';

export type Signal<T> = {
  get(this: void): T;
  subscribe(this: void, onUpdate: (value: T) => void): () => void;
};

export const useCreateSignal = <T>(value: T): Signal<T> => {
  const [subscriptions] = useState(() => new Set<(value: T) => void>());
  const valueRef = useLatestRef(value);
  const signal = useMemo<Signal<T>>(
    () => ({
      get: () => valueRef.current,
      subscribe: onUpdate => {
        subscriptions.add(onUpdate);
        return () => subscriptions.delete(onUpdate);
      },
    }),
    [valueRef, subscriptions]
  );

  useLayoutEffect(() => {
    subscriptions.forEach(subscription => subscription(value));
  }, [subscriptions, value]);

  return signal;
};

export const useSignal = <T>(signal: Signal<T>) =>
  useSyncExternalStore(signal.subscribe, signal.get);
