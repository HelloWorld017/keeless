import { Item, ItemContent, ItemDescription, ItemMedia, ItemTitle } from '@/components/item';
import { cn, cx } from '@/utils/css';
import { ItemIcon } from './ItemIcon';
import { Tag } from './Tag';
import type { EntrySummary, TagSummary } from '@keeless/schema';
import type { ComponentProps } from 'react';

export const getEntryTitle = (entry: EntrySummary) =>
  entry.name || (entry.nameIsProtected ? '(Protected Entry)' : '(Untitled Entry)');

export const EntryItem = ({
  entry,
  selected = false,
  className,
  variant,
  tags: tagCatalog = [],
  ...props
}: ComponentProps<typeof Item> & {
  entry: EntrySummary;
  selected?: boolean;
  tags?: TagSummary[];
}) => {
  const title = getEntryTitle(entry);
  const username = entry.username || (entry.usernameIsProtected ? 'Protected username' : undefined);
  const url = entry.url || (entry.urlIsProtected ? 'Protected URL' : undefined);
  const description = [username, url].filter(Boolean).join(' | ');

  return (
    <Item
      variant={variant ?? (selected ? 'muted' : 'default')}
      className={cn('h-16 flex-nowrap', selected && 'bg-primary', className)}
      {...props}
    >
      <ItemMedia variant="icon" className={cx(selected && 'text-primary-foreground')}>
        <ItemIcon icon={entry.icon} fallback="entry" />
      </ItemMedia>
      <ItemContent className="min-w-0 gap-0.5">
        <ItemTitle className={cx(selected && 'text-primary-foreground')}>{title}</ItemTitle>
        <ItemDescription className="flex min-h-5 items-center gap-1 overflow-hidden">
          {description && (
            <span
              className={cx('min-w-0 truncate break-all', selected && 'text-primary-foreground/75')}
            >
              {description}
            </span>
          )}
          {entry.tags.map(name => (
            <Tag
              key={name}
              name={name}
                  tagStyle={tagCatalog.find(tag => tag.name === name)?.style}
                  variant={selected ? 'transparent' : 'default'}
                  className='max-w-24'
                />
              ))}
        )}
      </ItemContent>
    </Item>
  );
};
