declare module 'react' {
  interface CSSProperties {
    [index: `--${string}`]: unknown;
  }
}

export {};
