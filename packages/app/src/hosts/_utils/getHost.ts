import { createBrowserHost } from '@/hosts/browser';
import type { Host, HostKind } from '@/types/Host';

type HostFactory = () => Host;

const hostPriority: readonly HostKind[] = ['desktop', 'extension', 'browser'];
const hostFactories: Partial<Record<HostKind, HostFactory>> = {
  browser: createBrowserHost,
};

export const getHosts = async (): Promise<Host[]> => {
  const hosts: Host[] = [];
  for (const kind of hostPriority) {
    const host = hostFactories[kind]?.();
    if (host && (await host.isAvailable())) {
      hosts.push(host);
    }
  }
  return hosts;
};
