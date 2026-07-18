declare global {
  type Empty = Record<string, never>;
  type IsNever<T> = [T] extends [never] ? true : false;
  type Simplify<T> = { [K in keyof T]: T[K] } & {};
}

export {};
