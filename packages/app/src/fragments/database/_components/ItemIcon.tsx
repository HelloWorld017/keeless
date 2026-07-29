import { useRequest } from '@/fragments/_providers/QueryProvider';
import { IconFile, IconFolder } from '@/icons';
import { cx } from '@/utils/css';
import { useState } from 'react';
import { standardIcons } from '../_constants/icons';
import type { IconReference } from '@keeless/schema';

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
      className={cx('size-[1em] object-contain', className)}
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

  const StandardIcon = standardIcons[icon.standardId]?.Icon;
  return StandardIcon ? (
    <StandardIcon className={className} aria-hidden="true" />
  ) : (
    <FallbackIcon fallback={fallback} className={className} />
  );
};
