import { WebDavSetup, type Host, type HostStorage } from '@keeless/app';
import { LocalFileSetup } from './components/LocalFileSetup';

const FileIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z" />
    <path d="M14 2v6h6" />
  </svg>
);

const CloudIcon = () => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
    <path d="M17.5 19H9a7 7 0 1 1 6.7-9.1A4.5 4.5 0 1 1 17.5 19Z" />
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
  {
    kind: 'webdav',
    label: 'WebDAV',
    description: 'Store the database on a WebDAV server.',
    icon: <CloudIcon />,
    setup: {
      title: 'Connect WebDAV',
      description: 'Enter the connection details for your server.',
      component: WebDavSetup,
    },
  },
];

export const desktopHost: Host = {
  id: 'desktop',
  kind: 'desktop',
  label: 'Desktop',
  storages,
  isAvailable: async () => true,
  connect: clientBundle => window.keelessDesktop.registerClient(clientBundle),
  send: frame => window.keelessDesktop.relayFrame(frame),
  onEntryFocus: listener => window.keelessDesktop.onEntryFocus(listener),
};
