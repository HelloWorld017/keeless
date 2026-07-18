import { getRequestClient } from '@/utils/request';
import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import type { OperationArgs, OperationName } from '@/utils/request';
import type { ReactNode } from 'react';

const requestClientQuery = {
  queryKey: ['request-client'] as const,
  queryFn: getRequestClient,
  staleTime: Infinity,
};

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

export const useRequestClient = () => useQuery(requestClientQuery);
export const useRequest = <TName extends OperationName>(
  name: TName,
  args: OperationArgs<TName>,
) => {
  const requestClient = useRequestClient();

  return useQuery({
    queryKey: ['request', name, args],
    queryFn: () => requestClient.data!.request(name, args),
    enabled: requestClient.isSuccess,
  });
};
