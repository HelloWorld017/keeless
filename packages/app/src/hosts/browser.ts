import type { Host } from '@/types/Host';
import type { BrowserCore } from '@keeless/host-browser';
import type { MessageFrame } from '@keeless/schema';

const encoder = new TextEncoder();
const decoder = new TextDecoder(undefined, { fatal: true });
const database = { provider: 'idb', path: 'keeless.kdbx' } as const;

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
    kind: 'browser',
    database,
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
      const response = await requireCore().handle(encoder.encode(JSON.stringify(frame)));
      if (!response) {
        return null;
      }
      return JSON.parse(decoder.decode(response)) as MessageFrame;
    },
  };
};
