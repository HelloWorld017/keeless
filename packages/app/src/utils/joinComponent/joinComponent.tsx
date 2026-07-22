import { Fragment } from 'react';
import type { ReactNode } from 'react';

export const joinComponent = (elements: ReactNode[], separator: ReactNode) =>
  elements.flatMap((element, index) =>
    index === 0 ? [element] : [<Fragment key={index}>{separator}</Fragment>, element],
  );
