import { buildContext } from '@/utils/context';
import { buildRoute } from '@/utils/route';
import { startTransition, useCallback, useDeferredValue, useMemo } from 'react';
import { Router, useLocation } from 'wouter';
import { useSearch, useBrowserLocation, useHistoryState } from 'wouter/use-browser-location';
import { z } from 'zod';
import type { RouteKind } from '@/utils/route';
import type { ReactNode } from 'react';
import type { AroundNavHandler } from 'wouter';

const historyStateSchema = z.object({
  length: z.int().nonnegative(),
  entries: z.array(z.string()).optional(),
});

const [RouterContextProvider, useRouterContext] = buildContext(
  ({
    fallback,
    historyLength,
    historyEntries,
  }: {
    fallback: RouteKind;
    historyLength: number;
    historyEntries: (string | null)[];
  }) => {
    const [, navigate] = useLocation();
    const historyBack = useCallback(() => {
      if (historyLength > 0) {
        history.back();
        return;
      }

      navigate(buildRoute(fallback), { replace: true });
    }, [fallback, historyLength, navigate]);

    const historyBackUntil = useCallback(
      (target: string) => {
        const currentIndex = historyEntries.length - 1;
        const targetIndex = historyEntries.lastIndexOf(target);
        if (targetIndex === currentIndex) {
          return;
        }

        if (targetIndex >= 0) {
          history.go(targetIndex - currentIndex);
          return;
        }

        navigate(target, { replace: true });
      },
      [historyEntries, navigate],
    );

    return {
      historyBack,
      historyBackUntil,
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
  const [location] = useBrowserLocation();
  const parsedHistoryState = useMemo(
    () => historyStateSchema.safeParse(historyState).data,
    [historyState],
  );

  const historyLength = parsedHistoryState?.length ?? 0;
  const historyEntries = useMemo(
    () =>
      parsedHistoryState?.entries?.length === historyLength + 1
        ? parsedHistoryState.entries
        : [...Array<string | null>(historyLength).fill(null), location],
    [historyLength, location, parsedHistoryState],
  );

  const aroundNav = useCallback<AroundNavHandler>(
    (navigate, to, options) => {
      const nextHistoryLength = historyLength + (options?.replace ? 0 : 1);
      const nextHistoryEntries = options?.replace
        ? [...historyEntries.slice(0, -1), to]
        : [...historyEntries, to];

      const nextHistoryState = z
        .looseObject({})
        .catch({})
        .transform(state => ({
          ...state,
          length: nextHistoryLength,
          entries: nextHistoryEntries,
        }))
        .parse(options?.state);

      if (typeof __DEV__ === 'boolean' && __DEV__) {
        // oxlint-disable-next-line no-console
        console.debug(`Routing to`, to, options, history.length);
      }

      startTransition(() => {
        navigate(to, {
          ...options,
          state: nextHistoryState,
        });
      });
    },
    [historyEntries, historyLength],
  );

  return (
    <Router
      aroundNav={aroundNav}
      hook={useBrowserLocationDeferred}
      searchHook={useBrowserSearchDeferred}
    >
      <RouterContextProvider
        fallback={fallback}
        historyLength={historyLength}
        historyEntries={historyEntries}
      >
        {children}
      </RouterContextProvider>
    </Router>
  );
};

export const useNavigate = () => useRouterContext(state => state.navigate);
export const useHistoryBack = () => useRouterContext(state => state.historyBack);
export const useHistoryBackUntil = () => useRouterContext(state => state.historyBackUntil);
