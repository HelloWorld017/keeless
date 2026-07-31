import { OperationMetadata, operationMetadata, OperationResource } from '@keeless/schema';
import { QueryClient } from '@tanstack/react-query';
import { isQueryKey } from './queryKey';

export const invalidateByResource = (
  queryClient: QueryClient,
  resources: readonly OperationResource[],
) => {
  if (!resources.length) {
    return;
  }

  const resourcesSet = new Set(resources);
  return queryClient.invalidateQueries({
    predicate: query => {
      if (!isQueryKey(query.queryKey)) {
        return false;
      }

      const operation = query.queryKey[1];
      const metadata: OperationMetadata | undefined = operationMetadata[operation];
      return metadata?.queries?.some(resource => resourcesSet.has(resource)) ?? false;
    },
  });
};
