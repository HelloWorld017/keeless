import { PasswordStep } from './PasswordStep';
import type { PasswordStepProps } from './PasswordStep';

export const UnlockStep = (props: PasswordStepProps) => <PasswordStep mode="unlock" {...props} />;
