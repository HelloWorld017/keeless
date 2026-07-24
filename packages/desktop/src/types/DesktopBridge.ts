import type { MessageFrame } from '@keeless/lesswire';

export interface DesktopBridge {
  registerClient(bundle: string): Promise<void>;
  relayFrame(frame: MessageFrame): Promise<MessageFrame | null>;
  pickLocalFile(mode: 'open' | 'create'): Promise<string | null>;
}
