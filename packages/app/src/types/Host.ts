import type { MessageFrame } from '@keeless/lesswire';
import type { OpenArgs } from '@keeless/schema';
import type { ComponentType, ReactNode } from 'react';

export type HostKind = 'desktop' | 'extension' | 'browser';

export type StorageProvider = Extract<OpenArgs, { storage: unknown }>['storage'];

export type StorageProviderGetter = () => StorageProvider | Promise<StorageProvider>;

export type StorageSetupComponentProps = {
  isPending: boolean;
  error?: string;
  onOpen: (getProvider: StorageProviderGetter) => Promise<void>;
};

export type HostStorageSetup =
  | {
      title?: string;
      description?: string;
      component: ComponentType<StorageSetupComponentProps>;
      getDefaultProvider?: never;
    }
  | {
      title?: string;
      description?: string;
      component: null;
      getDefaultProvider: StorageProviderGetter;
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
  connect(clientBundle: string): Promise<string>;
  send(frame: MessageFrame): Promise<MessageFrame | null>;
  onEntryFocus?(listener: (entryId: string) => void): () => void;
}
