import { Badge } from '@/components/badge';
import { IconTag, IconX } from '@/icons';
import { ItemIcon } from './ItemIcon';
import type { TagStyle } from '@keeless/schema';
import type { MouseEvent } from 'react';
import {cva, VariantProps} from 'class-variance-authority';

const tagVariants = cva(
  'max-w-full border-transparent px-1.5 transition-none',
  {
    variants: {
      variant: {
        default: 'bg-[color-mix(in_oklch,var(--tag-color)_16%,transparent)] text-[color-mix(in_oklch,var(--tag-color)_68%,black)] dark:bg-[color-mix(in_oklch,var(--tag-color)_22%,transparent)] dark:text-[color-mix(in_oklch,var(--tag-color)_72%,white)]',
        transparent: 'bg-transparent text-primary-foreground',
      },
      size: {
        default: 'gap-1',
        compact: 'gap-0.5 py-0 text-[0.6875rem]'
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'default',
    },
  }
);

type TagProps = {
  name: string;
  className?: string | undefined;
  tagStyle?: TagStyle | null;
  onRemove?: () => void;
} & (VariantProps<typeof tagVariants> & { class?: never; })

export const Tag = ({ name, tagStyle, onRemove, ...props }: TagProps) => {
  const remove = (event: MouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
    onRemove?.();
  };

  return (
    <Badge
      variant="secondary"
      className={tagVariants(props)}
      style={{ '--tag-color': tagStyle?.color ?? 'var(--color-primary)' }}
    >
      {tagStyle ? (
        <ItemIcon icon={tagStyle.icon} className='size-[1.2em]!' fallback="entry" />
      ) : (
        <IconTag aria-hidden="true" />
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
