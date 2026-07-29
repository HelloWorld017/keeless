import type { Host } from './Host';
import type { ComponentType } from 'react';

export type AppIntegrationExtraConfig = {
  category: string;
  component: ComponentType;
};

export type AppIntegration = {
  hostOverride?: Host;
  hasNativePasswordInput?: boolean;
  extraConfig?: readonly AppIntegrationExtraConfig[];
};
