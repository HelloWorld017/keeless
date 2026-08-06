import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogOverlay,
  DialogTitle,
} from '@/components/dialog';
import { cn } from '@/utils/css';
import type { ComponentProps } from 'react';

export const AdaptiveSheetContent = ({
  className,
  ...props
}: ComponentProps<typeof DialogContent>) => (
  <DialogContent
    className={cn(
      'top-auto right-0 bottom-0 left-0 max-w-none translate-x-0 translate-y-0 rounded-t-2xl sm:top-1/2 sm:right-auto sm:bottom-auto sm:left-1/2 sm:max-w-md sm:-translate-x-1/2 sm:-translate-y-1/2 sm:rounded-xl',
      className,
    )}
    {...props}
  />
);

export {
  Dialog as AdaptiveSheet,
  DialogHeader as AdaptiveSheetHeader,
  DialogTitle as AdaptiveSheetTitle,
  DialogDescription as AdaptiveSheetDescription,
  DialogOverlay as AdaptiveSheetOverlay,
};
