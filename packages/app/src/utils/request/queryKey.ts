import type { OperationArgs, OperationName } from './request';

export const queryKey = <TOperation extends OperationName>(
  operation: TOperation,
  args: OperationName extends TOperation ? unknown : OperationArgs<TOperation>,
) => ['request', operation, args] as const;

export const isQueryKey = (value: unknown): value is QueryKey =>
  Array.isArray(value) && value[0] === 'request';

export type QueryKey<TOperation extends OperationName = OperationName> = [
  'request',
  TOperation,
  OperationArgs<TOperation>,
];
