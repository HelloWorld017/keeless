import { getHosts } from '@/hosts';
import { buildContext } from '@/utils/context';
import { useCallback, useEffect, useState } from 'react';
import type { AppIntegration } from '@/types/AppIntegration';
import type { Host, HostKind } from '@/types/Host';

const [HostContextProvider, useHostContext] = buildContext(
  ({ integration }: { integration: AppIntegration }) => {
    const hostOverride = integration.hostOverride;
    const [hosts, setHosts] = useState<Host[]>(() => (hostOverride ? [hostOverride] : []));
    const [host, setHost] = useState<Host | undefined>(hostOverride);
    const [isLoading, setIsLoading] = useState(!hostOverride);

    useEffect(() => {
      if (hostOverride) {
        setHosts([hostOverride]);
        setHost(hostOverride);
        setIsLoading(false);
        return undefined;
      }

      let active = true;
      setHost(undefined);
      setIsLoading(true);
      void getHosts().then(nextHosts => {
        if (active) {
          setHost(nextHosts[0]);
          setHosts(nextHosts);
          setIsLoading(false);
        }
      });
      return () => {
        active = false;
      };
    }, [hostOverride]);

    const selectHost = useCallback(
      (kind: HostKind) => setHost(hosts.find(candidate => candidate.kind === kind)),
      [hosts],
    );

    return {
      host,
      hosts,
      isLoading,
      isOverride: Boolean(hostOverride),
      hasNativePasswordInput: Boolean(integration.hasNativePasswordInput),
      selectHost,
    };
  },
);

export const HostProvider = HostContextProvider;
export const useHost = () => useHostContext(state => state.host);
export const useHosts = () => useHostContext(state => state.hosts);
export const useHostsLoading = () => useHostContext(state => state.isLoading);
export const useHostOverride = () => useHostContext(state => state.isOverride);
export const useHasNativePasswordInput = () =>
  useHostContext(state => state.hasNativePasswordInput);
export const useSelectHost = () => useHostContext(state => state.selectHost);
