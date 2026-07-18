import { buildContext } from '@/utils/context';
import { buildRoute } from '@/utils/route';
import { startTransition, useCallback, useDeferredValue, useMemo } from 'react';
import { Router, useLocation } from 'wouter';
import { useSearch, useBrowserLocation, useHistoryState } from 'wouter/use-browser-location';
import { z } from 'zod';
import type { RouteKind } from '@/utils/route';
import type { ReactNode } from 'react';
import type { AroundNavHandler } from 'wouter';

const historyStateSchema = z.object({ length: z.int().positive() });

const [RouterContextProvider, useRouterContext] = buildContext(
  ({ fallback, historyLength }: { fallback: RouteKind; historyLength: number }) => {
    const [, navigate] = useLocation();
    const historyBack = useCallback(() => {
      if (historyLength > 0) {
        history.back();
        return;
      }

      navigate(buildRoute(fallback), { replace: true });
    }, [fallback, historyLength, navigate]);

    return {
      historyBack,
      navigate,
    };
  },
);

const useBrowserLocationDeferred = () => {
  const location = useBrowserLocation();
  return useDeferredValue(location);
};

const useBrowserSearchDeferred = () => {
  const search = useSearch();
  return useDeferredValue(search);
};

type RouterProviderProps = {
  fallback: RouteKind;
  children: ReactNode;
};

export const RouterProvider = ({ fallback, children }: RouterProviderProps) => {
  const historyState = useHistoryState<unknown>();
  const historyLength = useMemo(
    () => historyStateSchema.safeParse(historyState).data?.length ?? 0,
    [historyState],
  );

  const aroundNav = useCallback<AroundNavHandler>(
    (navigate, to, options) => {
      const nextHistoryLength = historyLength + (options?.replace ? 0 : 1);
      const nextHistoryState = z
        .looseObject({})
        .catch({})
        .transform(state => ({ ...state, length: nextHistoryLength }))
        .parse(options?.state);

      startTransition(() => {
        navigate(to, {
          ...options,
          state: nextHistoryState,
        });
      });
    },
    [historyLength],
  );

  return (
    <Router
      aroundNav={aroundNav}
      hook={useBrowserLocationDeferred}
      searchHook={useBrowserSearchDeferred}
    >
      <RouterContextProvider fallback={fallback} historyLength={historyLength}>
        {children}
      </RouterContextProvider>
    </Router>
  );
};

export const useNavigate = () => useRouterContext(state => state.navigate);
export const useHistoryBack = () => useRouterContext(state => state.historyBack);
