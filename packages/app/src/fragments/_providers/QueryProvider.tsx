import { getRequestClient } from '@/utils/request';
import { createContext, useCallback, useContext, useEffect, useState } from 'react';
import type { RequestClient } from '@/utils/request';
import type { ReactNode } from 'react';

type QueryState =
  | { status: 'loading'; client: null; error: null; retry: () => void }
  | { status: 'ready'; client: RequestClient; error: null; retry: () => void }
  | { status: 'error'; client: null; error: Error; retry: () => void };

const QueryContext = createContext<QueryState | null>(null);

export const QueryProvider = ({ children }: { children: ReactNode }) => {
  const [attempt, setAttempt] = useState(0);
  const retry = useCallback(() => setAttempt(value => value + 1), []);
  const [state, setState] = useState<QueryState>({
    status: 'loading',
    client: null,
    error: null,
    retry,
  });

  useEffect(() => {
    let active = true;
    setState({ status: 'loading', client: null, error: null, retry });
    void getRequestClient().then(
      client => {
        if (active) {
          setState({ status: 'ready', client, error: null, retry });
        }
      },
      error => {
        if (active) {
          setState({
            status: 'error',
            client: null,
            error: error instanceof Error ? error : new Error(String(error)),
            retry,
          });
        }
      },
    );
    return () => {
      active = false;
    };
  }, [attempt, retry]);

  return <QueryContext value={state}>{children}</QueryContext>;
};

export const useQueryState = () => {
  const state = useContext(QueryContext);
  if (!state) {
    throw new Error('useQueryState must be used inside QueryProvider');
  }
  return state;
};
