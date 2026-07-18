import type { Host, HostStorage } from '@/types/Host';
import type { BrowserCore } from '@keeless/host-browser';
import type { MessageFrame } from '@keeless/schema';

const encoder = new TextEncoder();
const decoder = new TextDecoder(undefined, { fatal: true });
const storages: readonly HostStorage[] = [
  {
    kind: 'indexeddb',
    label: 'IndexedDB',
    description: 'Store the database in this browser.',
    requiresDetails: false,
  },
  {
    kind: 'webdav',
    label: 'WebDAV',
    description: 'Store the database on a WebDAV server.',
    requiresDetails: true,
  },
];

const isBrowserAvailable = () =>
  typeof window !== 'undefined' &&
  typeof WebAssembly !== 'undefined' &&
  typeof indexedDB !== 'undefined';

export const createBrowserHost = (): Host => {
  let core: BrowserCore | undefined;

  const requireCore = () => {
    if (!core) {
      throw new Error('Browser host is not connected');
    }
    return core;
  };

  return {
    id: 'browser',
    kind: 'browser',
    label: 'Browser',
    storages,
    isAvailable: async () => isBrowserAvailable(),
    connect: async defaultApprovedBundle => {
      if (core) {
        return;
      }
      const browserHost = await import('@keeless/host-browser');
      await browserHost.default();
      core = await browserHost.BrowserCore.create(defaultApprovedBundle);
    },
    configureStorage: async input => {
      if (input.kind === 'indexeddb') {
        return { provider: 'idb', path: 'keeless.kdbx' };
      }
      await requireCore().configureWebDav(input.url, input.username, input.password);
      return {
        provider: 'webdav',
        path: input.path?.trim() || 'keeless.kdbx',
      };
    },
    send: async frame => {
      const response = await requireCore().handle(encoder.encode(JSON.stringify(frame)));
      if (!response) {
        return null;
      }
      return JSON.parse(decoder.decode(response)) as MessageFrame;
    },
  };
};
