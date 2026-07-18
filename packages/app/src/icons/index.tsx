import { AlertCircleIcon, ChevronLeftIcon, ChevronRightIcon, LoaderCircleIcon } from 'lucide-react';
import type { ComponentType } from 'react';

const wrapLucideComponent = <TProps,>(LucideIcon: ComponentType<TProps>) => {
  const IconComponent = (props: TProps) => {
    const Icon = LucideIcon as ComponentType<{ width: string; height: string }>;
    return <Icon width="1em" height="1em" stroke="currentColor" {...props} />;
  };

  IconComponent.displayName = LucideIcon.displayName && `Icon${LucideIcon.displayName}`;

  return IconComponent;
};

export const IconAlertCircle = wrapLucideComponent(AlertCircleIcon);
export const IconChevronLeft = wrapLucideComponent(ChevronLeftIcon);
export const IconChevronRight = wrapLucideComponent(ChevronRightIcon);
export const IconLoaderCircle = wrapLucideComponent(LoaderCircleIcon);
