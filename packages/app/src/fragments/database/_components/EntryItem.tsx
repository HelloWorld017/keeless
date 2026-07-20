import { Item, ItemContent, ItemDescription, ItemMedia, ItemTitle } from '@/components/item';
import { IconFile } from '@/icons';
import { cn, cx } from '@/utils/css';
import type { EntrySummary } from '@keeless/schema';
import type { ComponentProps } from 'react';

export const getEntryTitle = (entry: EntrySummary) =>
  entry.name || (entry.nameIsProtected ? '(Protected Entry)' : '(Untitled Entry)');

export const EntryItem = ({
  entry,
  selected = false,
  className,
  variant,
  ...props
}: ComponentProps<typeof Item> & {
  entry: EntrySummary;
  selected?: boolean;
}) => {
  const title = getEntryTitle(entry);
  const username = entry.username || (entry.usernameIsProtected ? 'Protected username' : undefined);
  const url = entry.url || (entry.urlIsProtected ? 'Protected URL' : undefined);
  const tags = entry.tags.map(tag => `#${tag}`).join(', ');
  const description = [url, username, tags || undefined].filter(Boolean).join(' | ');

  return (
    <Item
      variant={variant ?? (selected ? 'muted' : 'default')}
      className={cn('h-16 flex-nowrap', selected && 'bg-primary', className)}
      {...props}
    >
      <ItemMedia variant="icon">
        <IconFile />
      </ItemMedia>
      <ItemContent className="min-w-0 gap-0.5">
        <ItemTitle className={cx(selected && 'text-primary-foreground')}>{title}</ItemTitle>
        <ItemDescription className={cx('min-h-5 line-clamp-1', selected && 'text-primary-foreground/75')}>{description}</ItemDescription>
      </ItemContent>
    </Item>
  );
};
