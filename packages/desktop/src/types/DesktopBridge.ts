import type { MessageFrame } from '@keeless/lesswire';

export interface DesktopBridge {
  registerClient(bundle: string): Promise<string>;
  relayFrame(frame: MessageFrame): Promise<MessageFrame | null>;
  pickLocalFile(mode: 'open' | 'create'): Promise<string | null>;
  onEntryFocus(listener: (entryId: string) => void): () => void;
}
