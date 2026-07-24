import { contextBridge, ipcRenderer } from 'electron';
import type { MessageFrame } from '@keeless/lesswire';
import type { DesktopBridge } from '../shared/DesktopBridge';

const bridge: DesktopBridge = {
  registerClient: bundle => ipcRenderer.invoke('desktop:register-client', bundle),
  relayFrame: frame => ipcRenderer.invoke('desktop:relay-frame', frame) as Promise<MessageFrame | null>,
  pickLocalFile: mode => ipcRenderer.invoke('desktop:pick-local-file', mode),
};

contextBridge.exposeInMainWorld('keelessDesktop', bridge);
