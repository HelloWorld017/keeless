import { Alert, AlertDescription, AlertTitle } from '@/components/alert';
import { IconAlertCircle } from '@/icons';

export const StepError = ({ error }: { error?: string }) =>
  error && (
    <Alert variant="destructive">
      <IconAlertCircle />
      <AlertTitle>Could not continue</AlertTitle>
      <AlertDescription>{error}</AlertDescription>
    </Alert>
  );
