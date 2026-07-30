import { getRequestClient } from '@/utils/request';
import { operationMetadata, type OperationMetadata } from '@keeless/schema';
import {
  QueryClient,
  QueryClientProvider,
  useMutation,
  useQueryClient,
  useQuery,
  type UseMutationOptions,
  type UseQueryOptions,
} from '@tanstack/react-query';
import { useState } from 'react';
import { useHost } from './HostProvider';
import type { OperationArgs, OperationName, OperationResult } from '@/utils/request';
import type { ReactNode } from 'react';

export const QueryProvider = ({ children }: { children: ReactNode }) => {
  const [queryClient] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: { retry: false },
        },
      }),
  );

  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
};

export const useRequestClient = () => {
  const host = useHost();
  return useQuery({
    queryKey: ['request-client', host?.id],
    queryFn: () => getRequestClient(host!),
    enabled: Boolean(host),
    staleTime: Infinity,
  });
};

export const useRequest = <TName extends OperationName>(
  name: TName,
  args: OperationArgs<TName>,
  config: Omit<UseQueryOptions<OperationResult<TName>>, 'queryKey' | 'queryFn'> = {},
) => {
  const requestClient = useRequestClient();

  return useQuery({
    enabled: requestClient.isSuccess,
    ...config,
    queryKey: ['request', name, args],
    queryFn: () => requestClient.data!.request(name, args),
  });
};

type RequestMutationOptions<TName extends OperationName, TOnMutateResult> = Omit<
  UseMutationOptions<OperationResult<TName>, Error, OperationArgs<TName>, TOnMutateResult>,
  'mutationFn'
>;

export const useRequestMutation = <TName extends OperationName, TOnMutateResult = unknown>(
  name: TName,
  options: RequestMutationOptions<TName, TOnMutateResult> = {},
) => {
  const requestClient = useRequestClient();
  const queryClient = useQueryClient();
  const { onSuccess, ...mutationOptions } = options;
  const mutatedResources = (operationMetadata[name] as OperationMetadata).mutates;

  return useMutation({
    ...mutationOptions,
    mutationFn: (args: OperationArgs<TName>) => requestClient.data!.request(name, args),
    onSuccess: async (data, args, onMutateResult, context) => {
      if (mutatedResources?.length) {
        await queryClient.invalidateQueries({
          predicate: query => {
            const operation = query.queryKey[1];
            if (
              query.queryKey[0] !== 'request' ||
              typeof operation !== 'string' ||
              !(operation in operationMetadata)
            ) {
              return false;
            }

            return (
              (operationMetadata[operation as OperationName] as OperationMetadata).queries?.some(
                resource => mutatedResources.includes(resource),
              ) ?? false
            );
          },
        });
      }
      return onSuccess?.(data, args, onMutateResult, context);
    },
  });
};
