import type { MessageFrame, StorageDescriptor } from '@keeless/schema';

export type HostKind = 'desktop' | 'extension' | 'browser';

export interface Host {
  readonly kind: HostKind;
  readonly database: StorageDescriptor;
  isAvailable(): Promise<boolean>;
  connect(defaultApprovedBundle: string): Promise<void>;
  send(frame: MessageFrame): Promise<MessageFrame | null>;
}
