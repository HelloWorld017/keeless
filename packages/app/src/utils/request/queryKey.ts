import { OperationArgs, OperationName } from './request';

export const queryKey = <TOperation extends OperationName>(
  operation: TOperation,
  args: OperationName extends TOperation ? unknown : OperationArgs<TOperation>,
) => ['request', operation, args] as const;

export const isQueryKey = (queryKey: unknown): queryKey is QueryKey =>
  Array.isArray(queryKey) && queryKey[0] === 'request';

export type QueryKey<TOperation extends OperationName = OperationName> = [
  'request',
  TOperation,
  OperationArgs<TOperation>,
];
