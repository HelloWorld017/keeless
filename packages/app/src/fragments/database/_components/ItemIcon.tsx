import { useRequest } from '@/fragments/_providers/QueryProvider';
import { IconFile, IconFolder } from '@/icons';
import { cn } from '@/utils/css';
import {
  AppWindow,
  Apple,
  Archive,
  BadgeCheck,
  BatteryCharging,
  BookOpen,
  BookOpenText,
  Camera,
  CircleCheck,
  CircleDollarSign,
  ClipboardCheck,
  Clock,
  ClockAlert,
  Contact,
  Disc3,
  Feather,
  FileCheck,
  FileLock,
  FilePlus,
  FileQuestionMark,
  Flag,
  Folder,
  FolderCheck,
  FolderOpen,
  Globe,
  HardDrive,
  House,
  Images,
  Info,
  KeyRound,
  KeySquare,
  Landmark,
  List,
  LockKeyhole,
  LockOpen,
  Mail,
  Mailbox,
  MemoryStick,
  MessagesSquare,
  Monitor,
  MonitorCog,
  MonitorPlay,
  NotebookPen,
  Package,
  PackageOpen,
  PanelsTopLeft,
  PenLine,
  Play,
  Printer,
  Puzzle,
  Radio,
  RadioTower,
  Save,
  ScanLine,
  Search,
  Server,
  Settings,
  SlidersHorizontal,
  Smartphone,
  Sparkles,
  SquareTerminal,
  Star,
  StickyNote,
  Trash2,
  TriangleAlert,
  UserRoundKey,
  Wrench,
  Zap,
  type LucideIcon,
} from 'lucide-react';
import { useState } from 'react';
import type { IconReference } from '@keeless/schema';

// KeePass PwIcon values are positional, from Key (0) through BlackBerry (68).
const standardIcons: readonly LucideIcon[] = [
  KeyRound,
  Globe,
  TriangleAlert,
  Server,
  FolderCheck,
  MessagesSquare,
  Puzzle,
  NotebookPen,
  RadioTower,
  Contact,
  FileCheck,
  Camera,
  Radio,
  KeySquare,
  Zap,
  ScanLine,
  Sparkles,
  Disc3,
  Monitor,
  Mail,
  Settings,
  ClipboardCheck,
  FilePlus,
  MonitorPlay,
  BatteryCharging,
  Mailbox,
  Save,
  HardDrive,
  FileQuestionMark,
  LockKeyhole,
  SquareTerminal,
  Printer,
  AppWindow,
  Play,
  SlidersHorizontal,
  MonitorCog,
  Archive,
  Landmark,
  PanelsTopLeft,
  Clock,
  Search,
  Flag,
  MemoryStick,
  Trash2,
  StickyNote,
  ClockAlert,
  Info,
  Package,
  Folder,
  FolderOpen,
  PackageOpen,
  LockOpen,
  FileLock,
  CircleCheck,
  PenLine,
  Images,
  BookOpen,
  List,
  UserRoundKey,
  Wrench,
  House,
  Star,
  SquareTerminal,
  Feather,
  Apple,
  BookOpenText,
  CircleDollarSign,
  BadgeCheck,
  Smartphone,
];

type ItemIconProps = {
  icon: IconReference;
  fallback: 'entry' | 'group';
  className?: string;
};

const FallbackIcon = ({ fallback, className }: Pick<ItemIconProps, 'fallback' | 'className'>) => {
  const Icon = fallback === 'group' ? IconFolder : IconFile;
  return <Icon className={className} aria-hidden="true" />;
};

const CustomItemIcon = ({
  uuid,
  fallback,
  className,
}: Omit<ItemIconProps, 'icon'> & { uuid: string }) => {
  const icons = useRequest('getCustomIcons', {});
  const [loadFailed, setLoadFailed] = useState(false);
  const customIcon = icons.data?.icons.find(icon => icon.uuid.toLowerCase() === uuid.toLowerCase());

  if (!customIcon || loadFailed) {
    return <FallbackIcon fallback={fallback} className={className} />;
  }

  return (
    <img
      src={`data:image/png;base64,${customIcon.dataBase64}`}
      alt=""
      className={cn('size-[1em] object-contain', className)}
      onError={() => setLoadFailed(true)}
    />
  );
};

export const ItemIcon = ({ icon, fallback, className }: ItemIconProps) => {
  if (icon.customUuid) {
    return (
      <CustomItemIcon
        key={icon.customUuid}
        uuid={icon.customUuid}
        fallback={fallback}
        className={className}
      />
    );
  }

  const StandardIcon = standardIcons[icon.standardId];
  return StandardIcon ? (
    <StandardIcon className={className} width="1em" height="1em" aria-hidden="true" />
  ) : (
    <FallbackIcon fallback={fallback} className={className} />
  );
};
