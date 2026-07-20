import { useRequest } from '@/fragments/_providers/QueryProvider';
import {
  IconAppWindow,
  IconApple,
  IconArchive,
  IconBadgeCheck,
  IconBatteryCharging,
  IconBookOpen,
  IconBookOpenText,
  IconCamera,
  IconCircleCheck,
  IconCircleDollarSign,
  IconClipboardCheck,
  IconClock,
  IconClockAlert,
  IconContact,
  IconDisc3,
  IconFeather,
  IconFile,
  IconFileCheck,
  IconFileLock,
  IconFilePlus,
  IconFileQuestionMark,
  IconFlag,
  IconFolder,
  IconFolderCheck,
  IconFolderOpen,
  IconGlobe,
  IconHardDrive,
  IconHouse,
  IconImages,
  IconInfo,
  IconKeyRound,
  IconKeySquare,
  IconLandmark,
  IconList,
  IconLockKeyhole,
  IconLockOpen,
  IconMail,
  IconMailbox,
  IconMemoryStick,
  IconMessagesSquare,
  IconMonitor,
  IconMonitorCog,
  IconMonitorPlay,
  IconNotebookPen,
  IconPackage,
  IconPackageOpen,
  IconPanelsTopLeft,
  IconPenLine,
  IconPlay,
  IconPrinter,
  IconPuzzle,
  IconRadio,
  IconRadioTower,
  IconSave,
  IconScanLine,
  IconSearch,
  IconServer,
  IconSettings,
  IconSlidersHorizontal,
  IconSmartphone,
  IconSparkles,
  IconSquareTerminal,
  IconStar,
  IconStickyNote,
  IconTrash,
  IconTriangleAlert,
  IconUserRoundKey,
  IconWrench,
  IconZap,
} from '@/icons';
import { cn } from '@/utils/css';
import { useState } from 'react';
import type { IconReference } from '@keeless/schema';

// KeePass PwIcon values are positional, from Key (0) through BlackBerry (68).
const standardIcons = [
  IconKeyRound,
  IconGlobe,
  IconTriangleAlert,
  IconServer,
  IconFolderCheck,
  IconMessagesSquare,
  IconPuzzle,
  IconNotebookPen,
  IconRadioTower,
  IconContact,
  IconFileCheck,
  IconCamera,
  IconRadio,
  IconKeySquare,
  IconZap,
  IconScanLine,
  IconSparkles,
  IconDisc3,
  IconMonitor,
  IconMail,
  IconSettings,
  IconClipboardCheck,
  IconFilePlus,
  IconMonitorPlay,
  IconBatteryCharging,
  IconMailbox,
  IconSave,
  IconHardDrive,
  IconFileQuestionMark,
  IconLockKeyhole,
  IconSquareTerminal,
  IconPrinter,
  IconAppWindow,
  IconPlay,
  IconSlidersHorizontal,
  IconMonitorCog,
  IconArchive,
  IconLandmark,
  IconPanelsTopLeft,
  IconClock,
  IconSearch,
  IconFlag,
  IconMemoryStick,
  IconTrash,
  IconStickyNote,
  IconClockAlert,
  IconInfo,
  IconPackage,
  IconFolder,
  IconFolderOpen,
  IconPackageOpen,
  IconLockOpen,
  IconFileLock,
  IconCircleCheck,
  IconPenLine,
  IconImages,
  IconBookOpen,
  IconList,
  IconUserRoundKey,
  IconWrench,
  IconHouse,
  IconStar,
  IconSquareTerminal,
  IconFeather,
  IconApple,
  IconBookOpenText,
  IconCircleDollarSign,
  IconBadgeCheck,
  IconSmartphone,
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
    <StandardIcon className={className} aria-hidden="true" />
  ) : (
    <FallbackIcon fallback={fallback} className={className} />
  );
};
