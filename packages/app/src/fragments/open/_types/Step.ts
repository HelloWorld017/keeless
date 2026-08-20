export type OpenStep =
  | { kind: 'recent' }
  | { kind: 'select' }
  | { kind: 'storage'; storage: string }
  | { kind: 'create' }
  | { kind: 'unlock' };

export type OpenStepChange = (step: OpenStep, options?: { replace?: boolean }) => void;
