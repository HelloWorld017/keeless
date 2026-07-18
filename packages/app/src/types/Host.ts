import type { MessageFrame, StorageDescriptor } from '@keeless/schema';

export type HostKind = 'desktop' | 'extension' | 'browser';
export type HostStorageKind = 'indexeddb' | 'webdav';

export type HostStorage = {
  kind: HostStorageKind;
  label: string;
  description: string;
  requiresDetails: boolean;
};

export type HostStorageInput =
  | { kind: 'indexeddb' }
  | {
      kind: 'webdav';
      url: string;
      username: string;
      password: string;
      path?: string;
    };

export interface Host {
  readonly id: string;
  readonly kind: HostKind;
  readonly label: string;
  readonly storages: readonly HostStorage[];
  isAvailable(): Promise<boolean>;
  connect(defaultApprovedBundle: string): Promise<void>;
  configureStorage(input: HostStorageInput): Promise<StorageDescriptor>;
  send(frame: MessageFrame): Promise<MessageFrame | null>;
}
