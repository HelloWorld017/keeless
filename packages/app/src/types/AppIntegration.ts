import type { Host } from './Host';

export type PasswordInputMode = 'create' | 'unlock';

export type AppIntegration = {
  hostOverride?: Host;
  onPasswordInput?: (mode: PasswordInputMode) => Promise<string | null>;
};
