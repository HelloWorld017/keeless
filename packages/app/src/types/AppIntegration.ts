import type { Host } from './Host';

export type AppIntegration = {
  hostOverride?: Host;
  hasNativePasswordInput?: boolean;
};
