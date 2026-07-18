import type { MessageFrame, StorageDescriptor } from '@keeless/schema';

export type HostKind = 'desktop' | 'extension' | 'browser';

export type StoredDatabase = {
  descriptor: StorageDescriptor;
  name: string;
  size: number;
};

export interface HostStorage {
  readonly provider: string;
  readonly label: string;
  isAvailable(): Promise<boolean>;
  listDatabases(): Promise<StoredDatabase[]>;
  importDatabase(file: File): Promise<StoredDatabase>;
}

export interface Host {
  readonly kind: HostKind;
  readonly storages: readonly HostStorage[];
  isAvailable(): Promise<boolean>;
  connect(defaultApprovedBundle: string): Promise<void>;
  send(frame: MessageFrame): Promise<MessageFrame | null>;
}
