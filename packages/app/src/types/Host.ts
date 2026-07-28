import type { MessageFrame } from '@keeless/lesswire';
import type { StorageDescriptor } from '@keeless/schema';
import type { ComponentType, ReactNode } from 'react';

export type HostKind = 'desktop' | 'extension' | 'browser';

export type StorageDescriptorGetter = () => StorageDescriptor | Promise<StorageDescriptor>;

export type StorageSetupComponentProps = {
  isPending: boolean;
  error?: string;
  onBack: () => void;
  onOpen: (getDescriptor: StorageDescriptorGetter) => Promise<void>;
};

export type HostStorageSetup =
  | {
      title?: string;
      description?: string;
      component: ComponentType<StorageSetupComponentProps>;
      getDefaultDescriptor?: never;
    }
  | {
      title?: string;
      description?: string;
      component: null;
      getDefaultDescriptor: StorageDescriptorGetter;
    };

export type HostStorage = {
  kind: string;
  icon: ReactNode;
  label: string;
  description: string;
  setup: HostStorageSetup;
};

export interface Host {
  readonly id: string;
  readonly kind: HostKind;
  readonly label: string;
  readonly storages: readonly HostStorage[];
  isAvailable(): Promise<boolean>;
  connect(defaultApprovedBundle: string): Promise<void>;
  send(frame: MessageFrame): Promise<MessageFrame | null>;
  onEntryFocus?(listener: (entryId: string) => void): () => void;
}
