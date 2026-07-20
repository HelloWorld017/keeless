import { Alert, AlertAction, AlertDescription } from '@/components/alert';
import { Button } from '@/components/button';
import { useDismissToast, useToasts } from '@/fragments/_providers/ToastProvider';
import { IconInfo, IconTriangleAlert, IconX } from '@/icons';
import { cn } from '@/utils/css';
import type { ToastItem } from '@/fragments/_providers/ToastProvider';

type ToastProps = {
  toast: ToastItem;
  onDismiss: (id: number) => void;
};

const Toast = ({ toast, onDismiss }: ToastProps) => {
  const destructive = toast.kind === 'destructive';
  const Icon = destructive ? IconTriangleAlert : IconInfo;

  return (
    <Alert
      role={destructive ? 'alert' : 'status'}
      aria-atomic="true"
      variant={destructive ? 'destructive' : 'default'}
      className={cn(
        'pointer-events-auto animate-in shadow-lg duration-200 fade-in slide-in-from-bottom-2',
        toast.kind === 'primary' &&
          'border-primary/30 bg-primary/10 text-primary *:data-[slot=alert-description]:text-primary',
      )}
    >
      <Icon aria-hidden="true" />
      <AlertDescription>{toast.message}</AlertDescription>
      <AlertAction>
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-label={`Dismiss notification: ${toast.message}`}
          onClick={() => onDismiss(toast.id)}
          className={cn(
            destructive && 'text-destructive hover:text-destructive',
            toast.kind === 'primary' && 'text-primary hover:text-primary',
          )}
        >
          <IconX aria-hidden="true" />
        </Button>
      </AlertAction>
    </Alert>
  );
};

export const ToastList = () => {
  const toasts = useToasts();
  const dismissToast = useDismissToast();

  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-0 z-50 px-4 pb-4 sm:px-6 sm:pb-6">
      <div className="mx-auto w-full max-w-md">
        <div className="relative flex flex-col gap-2">
          {toasts.map(toast => (
            <Toast key={toast.id} toast={toast} onDismiss={dismissToast} />
          ))}
        </div>
      </div>
    </div>
  );
};
