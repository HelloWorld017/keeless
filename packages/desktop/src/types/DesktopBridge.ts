import type { MessageFrame } from '@keeless/lesswire';

export type WindowControlAction = 'minimize' | 'maximize' | 'close';

export type PasskeyState = {
  platform: 'linux' | 'windows';
  state: 'enabled' | 'disabled' | 'degraded' | 'unsupported';
  enabled: boolean;
  checks: PasskeyCheck[];
};

export type PasskeyCheck = {
  id: string;
  label: string;
  status: 'ok' | 'warning' | 'error';
  detail?: string;
};

export interface DesktopBridge {
  registerClient(bundle: string): Promise<string>;
  relayFrame(frame: MessageFrame): Promise<MessageFrame | null>;
  pickLocalFile(mode: 'open' | 'create'): Promise<string | null>;
  windowControl(action: WindowControlAction): Promise<void>;
  getPasskeyState(): Promise<PasskeyState>;
  setPasskeyEnabled(enabled: boolean): Promise<PasskeyState>;
  onEntryFocus(listener: (entryId: string) => void): () => void;
}
