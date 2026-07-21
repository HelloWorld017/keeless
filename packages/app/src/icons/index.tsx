import {
  AlertCircleIcon,
  AppWindowIcon,
  ArrowRightIcon,
  AppleIcon,
  ArchiveIcon,
  BadgeCheckIcon,
  BatteryChargingIcon,
  BookOpenIcon,
  BookOpenTextIcon,
  CameraIcon,
  CalendarIcon,
  CheckIcon,
  ChevronDownIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  CircleCheckIcon,
  CircleDollarSignIcon,
  ClipboardCheckIcon,
  ClockAlertIcon,
  ClockIcon,
  CloudIcon,
  ContactIcon,
  CopyIcon,
  DatabaseIcon,
  DatabaseZapIcon,
  Disc3Icon,
  EllipsisVerticalIcon,
  EyeIcon,
  EyeOffIcon,
  FeatherIcon,
  FileCheckIcon,
  FileIcon,
  FileLockIcon,
  FilePlusIcon,
  FileQuestionMarkIcon,
  FlagIcon,
  FolderCheckIcon,
  FolderIcon,
  FolderOpenIcon,
  GlobeIcon,
  GripVerticalIcon,
  HardDriveIcon,
  HouseIcon,
  ImagesIcon,
  InfoIcon,
  KeyRoundIcon,
  KeySquareIcon,
  LandmarkIcon,
  ListIcon,
  LoaderCircleIcon,
  LockKeyholeIcon,
  LockKeyholeOpenIcon,
  LockOpenIcon,
  MailIcon,
  MailboxIcon,
  MemoryStickIcon,
  MessagesSquareIcon,
  MonitorCogIcon,
  MonitorIcon,
  MonitorPlayIcon,
  NotebookPenIcon,
  PackageIcon,
  PackageOpenIcon,
  PanelLeftIcon,
  PanelsTopLeftIcon,
  PenLineIcon,
  PencilIcon,
  PlayIcon,
  PlusIcon,
  PrinterIcon,
  PuzzleIcon,
  RadioIcon,
  RadioTowerIcon,
  RefreshCwIcon,
  SaveIcon,
  ScanLineIcon,
  SearchIcon,
  ServerIcon,
  SettingsIcon,
  SlidersHorizontalIcon,
  SmartphoneIcon,
  SparklesIcon,
  SquareTerminalIcon,
  StarIcon,
  StickyNoteIcon,
  TagIcon,
  Trash2Icon,
  TriangleAlertIcon,
  UserRoundKeyIcon,
  WrenchIcon,
  XIcon,
  ZapIcon,
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
export const IconAppWindow = wrapLucideComponent(AppWindowIcon);
export const IconArrowRight = wrapLucideComponent(ArrowRightIcon);
export const IconApple = wrapLucideComponent(AppleIcon);
export const IconArchive = wrapLucideComponent(ArchiveIcon);
export const IconBadgeCheck = wrapLucideComponent(BadgeCheckIcon);
export const IconBatteryCharging = wrapLucideComponent(BatteryChargingIcon);
export const IconBookOpen = wrapLucideComponent(BookOpenIcon);
export const IconBookOpenText = wrapLucideComponent(BookOpenTextIcon);
export const IconCamera = wrapLucideComponent(CameraIcon);
export const IconCalendar = wrapLucideComponent(CalendarIcon);
export const IconCheck = wrapLucideComponent(CheckIcon);
export const IconChevronDown = wrapLucideComponent(ChevronDownIcon);
export const IconChevronLeft = wrapLucideComponent(ChevronLeftIcon);
export const IconChevronRight = wrapLucideComponent(ChevronRightIcon);
export const IconCircleCheck = wrapLucideComponent(CircleCheckIcon);
export const IconCircleDollarSign = wrapLucideComponent(CircleDollarSignIcon);
export const IconClipboardCheck = wrapLucideComponent(ClipboardCheckIcon);
export const IconClock = wrapLucideComponent(ClockIcon);
export const IconClockAlert = wrapLucideComponent(ClockAlertIcon);
export const IconCloud = wrapLucideComponent(CloudIcon);
export const IconContact = wrapLucideComponent(ContactIcon);
export const IconCopy = wrapLucideComponent(CopyIcon);
export const IconDatabase = wrapLucideComponent(DatabaseIcon);
export const IconDatabaseZap = wrapLucideComponent(DatabaseZapIcon);
export const IconDisc3 = wrapLucideComponent(Disc3Icon);
export const IconEllipsisVertical = wrapLucideComponent(EllipsisVerticalIcon);
export const IconEye = wrapLucideComponent(EyeIcon);
export const IconEyeOff = wrapLucideComponent(EyeOffIcon);
export const IconFeather = wrapLucideComponent(FeatherIcon);
export const IconFile = wrapLucideComponent(FileIcon);
export const IconFileCheck = wrapLucideComponent(FileCheckIcon);
export const IconFileLock = wrapLucideComponent(FileLockIcon);
export const IconFilePlus = wrapLucideComponent(FilePlusIcon);
export const IconFileQuestionMark = wrapLucideComponent(FileQuestionMarkIcon);
export const IconFlag = wrapLucideComponent(FlagIcon);
export const IconFolder = wrapLucideComponent(FolderIcon);
export const IconFolderCheck = wrapLucideComponent(FolderCheckIcon);
export const IconFolderOpen = wrapLucideComponent(FolderOpenIcon);
export const IconGlobe = wrapLucideComponent(GlobeIcon);
export const IconGripVertical = wrapLucideComponent(GripVerticalIcon);
export const IconHardDrive = wrapLucideComponent(HardDriveIcon);
export const IconHouse = wrapLucideComponent(HouseIcon);
export const IconImages = wrapLucideComponent(ImagesIcon);
export const IconInfo = wrapLucideComponent(InfoIcon);
export const IconKeyRound = wrapLucideComponent(KeyRoundIcon);
export const IconKeySquare = wrapLucideComponent(KeySquareIcon);
export const IconLandmark = wrapLucideComponent(LandmarkIcon);
export const IconList = wrapLucideComponent(ListIcon);
export const IconLoaderCircle = wrapLucideComponent(LoaderCircleIcon);
export const IconLockKeyhole = wrapLucideComponent(LockKeyholeIcon);
export const IconLockKeyholeOpen = wrapLucideComponent(LockKeyholeOpenIcon);
export const IconLockOpen = wrapLucideComponent(LockOpenIcon);
export const IconMail = wrapLucideComponent(MailIcon);
export const IconMailbox = wrapLucideComponent(MailboxIcon);
export const IconMemoryStick = wrapLucideComponent(MemoryStickIcon);
export const IconMessagesSquare = wrapLucideComponent(MessagesSquareIcon);
export const IconMonitor = wrapLucideComponent(MonitorIcon);
export const IconMonitorCog = wrapLucideComponent(MonitorCogIcon);
export const IconMonitorPlay = wrapLucideComponent(MonitorPlayIcon);
export const IconNotebookPen = wrapLucideComponent(NotebookPenIcon);
export const IconPackage = wrapLucideComponent(PackageIcon);
export const IconPackageOpen = wrapLucideComponent(PackageOpenIcon);
export const IconPanelLeft = wrapLucideComponent(PanelLeftIcon);
export const IconPanelsTopLeft = wrapLucideComponent(PanelsTopLeftIcon);
export const IconPenLine = wrapLucideComponent(PenLineIcon);
export const IconPencil = wrapLucideComponent(PencilIcon);
export const IconPlay = wrapLucideComponent(PlayIcon);
export const IconPlus = wrapLucideComponent(PlusIcon);
export const IconPrinter = wrapLucideComponent(PrinterIcon);
export const IconPuzzle = wrapLucideComponent(PuzzleIcon);
export const IconRadio = wrapLucideComponent(RadioIcon);
export const IconRadioTower = wrapLucideComponent(RadioTowerIcon);
export const IconRefreshCw = wrapLucideComponent(RefreshCwIcon);
export const IconSave = wrapLucideComponent(SaveIcon);
export const IconScanLine = wrapLucideComponent(ScanLineIcon);
export const IconSearch = wrapLucideComponent(SearchIcon);
export const IconServer = wrapLucideComponent(ServerIcon);
export const IconSettings = wrapLucideComponent(SettingsIcon);
export const IconSlidersHorizontal = wrapLucideComponent(SlidersHorizontalIcon);
export const IconSmartphone = wrapLucideComponent(SmartphoneIcon);
export const IconSparkles = wrapLucideComponent(SparklesIcon);
export const IconSquareTerminal = wrapLucideComponent(SquareTerminalIcon);
export const IconStar = wrapLucideComponent(StarIcon);
export const IconStickyNote = wrapLucideComponent(StickyNoteIcon);
export const IconTag = wrapLucideComponent(TagIcon);
export const IconTrash = wrapLucideComponent(Trash2Icon);
export const IconTriangleAlert = wrapLucideComponent(TriangleAlertIcon);
export const IconUserRoundKey = wrapLucideComponent(UserRoundKeyIcon);
export const IconWrench = wrapLucideComponent(WrenchIcon);
export const IconX = wrapLucideComponent(XIcon);
export const IconZap = wrapLucideComponent(ZapIcon);
