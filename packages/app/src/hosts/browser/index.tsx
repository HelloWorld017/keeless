import { IconCloud, IconDatabaseZap, IconFile } from '@/icons';
import { LocalFileSetup } from './_components/LocalFileSetup';
import { WebDavSetup } from './_components/WebDavSetup';
import type { Host, HostStorage } from '@/types/Host';
import type { BrowserCore } from '@keeless/host-browser';
import type { MessageFrame } from '@keeless/lesswire';

const encoder = new TextEncoder();
const decoder = new TextDecoder(undefined, { fatal: true });
const isBrowserAvailable = () =>
  typeof window !== 'undefined' &&
  typeof WebAssembly !== 'undefined' &&
  typeof indexedDB !== 'undefined';

export const createBrowserHost = (): Host => {
  let core: BrowserCore | undefined;

  if (
    typeof __KEELESS_BROWSER_HOST_DISABLED__ !== 'undefined' &&
    __KEELESS_BROWSER_HOST_DISABLED__
  ) {
    throw new Error('browser host is disabled!');
  }

  const requireCore = () => {
    if (!core) {
      throw new Error('Browser host is not connected');
    }
    return core;
  };

  const storages: readonly HostStorage[] = [
    {
      kind: 'indexeddb',
      label: 'IndexedDB',
      description: 'Store the database in this browser.',
      icon: <IconDatabaseZap />,
      setup: {
        component: null,
        getDefaultDescriptor: () => ({ provider: 'indexeddb', path: '' }),
      },
    },
    {
      kind: 'local-file',
      label: 'Local file',
      description: 'Open a KeePass database from this device.',
      icon: <IconFile />,
      setup: {
        title: 'Open a local file',
        description: 'Choose or drop an existing KeePass database.',
        component: props => <LocalFileSetup {...props} getCore={requireCore} />,
      },
    },
    {
      kind: 'webdav',
      label: 'WebDAV',
      description: 'Store the database on a WebDAV server.',
      icon: <IconCloud />,
      setup: {
        title: 'Connect WebDAV',
        description: 'Enter the connection details for your server.',
        component: props => <WebDavSetup {...props} getCore={requireCore} />,
      },
    },
  ];

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
    send: async frame => {
      const response = await requireCore().handle(encoder.encode(JSON.stringify(frame)));
      if (!response) {
        return null;
      }
      return JSON.parse(decoder.decode(response)) as MessageFrame;
    },
  };
};
