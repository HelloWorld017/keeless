import type { Host, HostStorage } from '@/types/Host';
import type { BrowserCore } from '@keeless/host-browser';
import type { MessageFrame } from '@keeless/schema';

const encoder = new TextEncoder();
const decoder = new TextDecoder(undefined, { fatal: true });

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

  const indexedDbStorage: HostStorage = {
    provider: 'idb',
    label: 'This browser',
    isAvailable: async () => isBrowserAvailable(),
    listDatabases: async () =>
      (await requireCore().listDatabases()).map(database => ({
        descriptor: { provider: 'idb', path: database.path },
        name: database.name,
        size: database.size,
      })),
    importDatabase: async file => {
      const bytes = new Uint8Array(await file.arrayBuffer());
      try {
        const path = await requireCore().importDatabase(file.name, bytes);
        return {
          descriptor: { provider: 'idb', path },
          name: file.name,
          size: file.size,
        };
      } finally {
        bytes.fill(0);
      }
    },
  };

  return {
    kind: 'browser',
    storages: [indexedDbStorage],
    isAvailable: async () => isBrowserAvailable(),
    connect: async defaultApprovedBundle => {
      if (core) {
        return;
      }
      const browserHost = await import('@keeless/host-browser');
      await browserHost.default();
      core = await browserHost.BrowserCore.create(defaultApprovedBundle);
    },
    send: async frame => {
      const response = await requireCore().processFrame(encoder.encode(JSON.stringify(frame)));
      if (!response) {
        return null;
      }
      return JSON.parse(decoder.decode(response)) as MessageFrame;
    },
  };
};
