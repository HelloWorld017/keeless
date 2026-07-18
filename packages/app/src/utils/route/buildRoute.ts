type ExtractParamName<Segment extends string> = Segment extends `:${infer ParamName}`
  ? ParamName
  : never;

type ExtractParams<Path extends string> = Path extends `${infer SegmentA}/${infer SegmentB}`
  ? ExtractParamName<SegmentA> | ExtractParams<SegmentB>
  : ExtractParamName<Path>;

export const RouteMap = {
  database: '/',
  group: '/group/:group',
  tag: '/tag/:tag',
  config: '/config',
  open: '/open',
} as const;

export type RouteParams = Simplify<{
  [K in keyof typeof RouteMap]: Record<ExtractParams<(typeof RouteMap)[K]>, string>;
}>;

export type RouteKind = keyof typeof RouteMap;

export const getRoute = (kind: RouteKind, nest = false): string =>
  `${RouteMap[kind]}${nest ? '/*?' : ''}`;

export const buildRoute = <TKind extends RouteKind>(
  kind: TKind,
  ...[params]: IsNever<keyof RouteParams[TKind]> extends true ? [] : [RouteParams[TKind]]
) => {
  const baseRoute = getRoute(kind);
  if (!params) {
    return baseRoute;
  }

  return Object.entries<string>(params).reduce(
    (route, [key, value]) => route.replace(`:${key}`, value),
    baseRoute,
  );
};
