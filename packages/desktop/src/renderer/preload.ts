import { contextBridge, ipcRenderer } from 'electron';
import type { DesktopBridge } from '../types/DesktopBridge';
import type { MessageFrame } from '@keeless/lesswire';

const bridge: DesktopBridge = {
  registerClient: bundle => ipcRenderer.invoke('desktop:register-client', bundle),
  relayFrame: frame =>
    ipcRenderer.invoke('desktop:relay-frame', frame) as Promise<MessageFrame | null>,
  pickLocalFile: mode => ipcRenderer.invoke('desktop:pick-local-file', mode),
  onEntryFocus: listener => {
    const handler = (_event: Electron.IpcRendererEvent, entryId: unknown) => {
      if (typeof entryId === 'string') {
        listener(entryId);
      }
    };
    ipcRenderer.on('desktop:focus-entry', handler);
    return () => ipcRenderer.removeListener('desktop:focus-entry', handler);
  },
};

contextBridge.exposeInMainWorld('keelessDesktop', bridge);
