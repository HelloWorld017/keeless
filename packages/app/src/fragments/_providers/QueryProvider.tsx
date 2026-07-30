import { getRequestClient } from '@/utils/request';
import { QueryClient, QueryClientProvider, useQuery, UseQueryOptions } from '@tanstack/react-query';
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
