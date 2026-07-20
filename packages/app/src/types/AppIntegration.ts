import type { Host } from './Host';

export type PasswordInputMode = 'create' | 'unlock' | 'reveal' | 'save';

export type AppIntegration = {
  hostOverride?: Host;
  onPasswordInput?: (mode: PasswordInputMode) => Promise<string | null>;
};
