import {
  AlertCircleIcon,
  ArrowRightIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  CloudIcon,
  DatabaseIcon,
  DatabaseZapIcon,
  EyeIcon,
  EyeOffIcon,
  FileIcon,
  FolderIcon,
  GripVerticalIcon,
  ListIcon,
  LoaderCircleIcon,
  PanelLeftIcon,
  PencilIcon,
  PlusIcon,
  TagIcon,
  Trash2Icon,
  XIcon,
} from 'lucide-react';
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
export const IconArrowRight = wrapLucideComponent(ArrowRightIcon);
export const IconChevronLeft = wrapLucideComponent(ChevronLeftIcon);
export const IconChevronRight = wrapLucideComponent(ChevronRightIcon);
export const IconCloud = wrapLucideComponent(CloudIcon);
export const IconDatabase = wrapLucideComponent(DatabaseIcon);
export const IconDatabaseZap = wrapLucideComponent(DatabaseZapIcon);
export const IconEye = wrapLucideComponent(EyeIcon);
export const IconEyeOff = wrapLucideComponent(EyeOffIcon);
export const IconFile = wrapLucideComponent(FileIcon);
export const IconFolder = wrapLucideComponent(FolderIcon);
export const IconGripVertical = wrapLucideComponent(GripVerticalIcon);
export const IconList = wrapLucideComponent(ListIcon);
export const IconLoaderCircle = wrapLucideComponent(LoaderCircleIcon);
export const IconPanelLeft = wrapLucideComponent(PanelLeftIcon);
export const IconPencil = wrapLucideComponent(PencilIcon);
export const IconPlus = wrapLucideComponent(PlusIcon);
export const IconTag = wrapLucideComponent(TagIcon);
export const IconTrash = wrapLucideComponent(Trash2Icon);
export const IconX = wrapLucideComponent(XIcon);
