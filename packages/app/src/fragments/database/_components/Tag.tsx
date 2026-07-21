import { Badge } from '@/components/badge';
import { IconTag, IconX } from '@/icons';
import { cn } from '@/utils/css';
import { ItemIcon } from './ItemIcon';
import type { TagStyle } from '@keeless/schema';
import type { CSSProperties, MouseEvent } from 'react';

type TagProps = {
  name: string;
  style?: TagStyle | null;
  compact?: boolean;
  className?: string;
  onRemove?: () => void;
};

export const Tag = ({ name, style, compact = false, className, onRemove }: TagProps) => {
  const remove = (event: MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
    onRemove?.();
  };

  return (
    <Badge
      variant={style ? 'outline' : 'secondary'}
      className={cn(
        'max-w-full border-transparent',
        compact ? 'gap-0.5 px-1.5 py-0 text-[0.6875rem]' : 'gap-1',
        style &&
          'border-[color-mix(in_oklch,var(--tag-color)_30%,transparent)] bg-[color-mix(in_oklch,var(--tag-color)_16%,transparent)] text-[color-mix(in_oklch,var(--tag-color)_68%,black)] dark:bg-[color-mix(in_oklch,var(--tag-color)_22%,transparent)] dark:text-[color-mix(in_oklch,var(--tag-color)_72%,white)]',
        className,
      )}
      style={style ? ({ '--tag-color': style.color } as CSSProperties) : undefined}
    >
      {style ? (
        <ItemIcon icon={style.icon} fallback="entry" className="size-3" />
      ) : (
        <IconTag className="size-3" aria-hidden="true" />
      )}
      <span className="truncate">{name}</span>
      {onRemove && (
        <button
          type="button"
          className="-mr-1 inline-flex size-4 items-center justify-center rounded-sm opacity-60 outline-none hover:opacity-100 focus-visible:ring-2 focus-visible:ring-current"
          aria-label={`Remove ${name} tag`}
          onClick={remove}
        >
          <IconX className="size-3" />
        </button>
      )}
    </Badge>
  );
};
