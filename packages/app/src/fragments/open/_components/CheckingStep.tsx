import { Button } from '@/components/button';
import { IconLoaderCircle } from '@/icons';
import { SetupLayout } from './SetupLayout';
import { StepError } from './StepError';

export const CheckingStep = ({
  isPending,
  error,
  onRetry,
}: {
  isPending: boolean;
  error?: string;
  onRetry: () => void;
}) => (
  <SetupLayout title="Opening database" description="Checking the selected host." showBack={false}>
    <div className="flex items-center gap-2 text-sm text-muted-foreground">
      {isPending && <IconLoaderCircle className="animate-spin" />}
      {isPending ? 'Checking database status...' : 'Database status could not be checked.'}
    </div>
    <StepError error={error} />
    {error && (
      <Button type="button" variant="outline" onClick={onRetry}>
        Try again
      </Button>
    )}
  </SetupLayout>
);
