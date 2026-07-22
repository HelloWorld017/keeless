import { invoke } from '@tauri-apps/api/core';
import { LocalFileSetup } from './components/LocalFileSetup';
import type { Host, HostStorage } from '@keeless/app';
import type { MessageFrame } from '@keeless/schema';

type EnsureDaemonResult = 'connected' | 'started' | 'restarted';

let approvedBundle: string | undefined;
let hasConnected = false;

const waitForReload = async () => {
  window.location.reload();
  await new Promise<never>(() => undefined);
};

const ensureDaemon = async () => {
  if (!approvedBundle) {
    throw new Error('Desktop host is not connected');
  }
  return invoke<EnsureDaemonResult>('ensure_daemon', {
    defaultApprovedBundle: approvedBundle,
  });
};

const FileIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z" />
    <path d="M14 2v6h6" />
  </svg>
);

const storages: readonly HostStorage[] = [
  {
    kind: 'local-file',
    label: 'Local file',
    description: 'Open or create a KeePass database on this device.',
    icon: <FileIcon />,
    setup: {
      title: 'Choose a local database',
      description: 'Open an existing .kdbx database or choose where to create one.',
      component: LocalFileSetup,
    },
  },
];

export const desktopHost: Host = {
  id: 'desktop',
  kind: 'desktop',
  label: 'Desktop',
  storages,
  isAvailable: async () => true,
  connect: async defaultApprovedBundle => {
    approvedBundle = defaultApprovedBundle;
    const result = await ensureDaemon();
    if (result === 'restarted') {
      await waitForReload();
    }
    hasConnected = true;
  },
  send: async frame => {
    try {
      return await invoke<MessageFrame | null>('relay_frame', { frame });
    } catch (error) {
      const result = await ensureDaemon();
      if (hasConnected && result !== 'connected') {
        await waitForReload();
      }
      throw error;
    }
  },
};
