import type { MessageFrame } from '@keeless/lesswire';

export type WindowControlAction = 'minimize' | 'maximize' | 'close';

export interface DesktopBridge {
  registerClient(bundle: string): Promise<string>;
  relayFrame(frame: MessageFrame): Promise<MessageFrame | null>;
  pickLocalFile(mode: 'open' | 'create'): Promise<string | null>;
  windowControl(action: WindowControlAction): Promise<void>;
  onEntryFocus(listener: (entryId: string) => void): () => void;
}
