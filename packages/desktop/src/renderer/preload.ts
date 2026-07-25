import { contextBridge, ipcRenderer } from 'electron';
import type { DesktopBridge } from '../types/DesktopBridge';
import type { MessageFrame } from '@keeless/lesswire';

const bridge: DesktopBridge = {
  registerClient: bundle => ipcRenderer.invoke('desktop:register-client', bundle),
  relayFrame: frame =>
    ipcRenderer.invoke('desktop:relay-frame', frame) as Promise<MessageFrame | null>,
  pickLocalFile: mode => ipcRenderer.invoke('desktop:pick-local-file', mode),
};

contextBridge.exposeInMainWorld('keelessDesktop', bridge);
