import { buildContext } from '@/utils/context';
import type { AppIntegration, AppIntegrationExtraConfig } from '@/types/AppIntegration';

const emptyExtraConfig: readonly AppIntegrationExtraConfig[] = [];

const [AppIntegrationContextProvider, useAppIntegrationContext] = buildContext(
  ({ integration }: { integration: AppIntegration }) => integration,
);

export const AppIntegrationProvider = AppIntegrationContextProvider;
export const useExtraConfig = () =>
  useAppIntegrationContext(integration => integration.extraConfig ?? emptyExtraConfig);
