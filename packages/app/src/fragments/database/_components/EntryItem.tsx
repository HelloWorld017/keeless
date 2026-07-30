import { Item, ItemContent, ItemDescription, ItemMedia, ItemTitle } from '@/components/item';
import { cx } from '@/utils/css';
import { joinComponent } from '@/utils/joinComponent';
import { ItemIcon } from './ItemIcon';
import { Tag } from './Tag';
import type { EntrySummary, TagSummary } from '@keeless/schema';
import type { ComponentProps } from 'react';

export const getEntryTitle = (entry: EntrySummary) =>
  entry.name || (entry.nameIsProtected ? '(Protected Entry)' : '(Untitled Entry)');

export const getEntryItemSize = (entry: EntrySummary) => {
  let height = 10;
  if (entry.username || entry.url) {
    height += 6;
  }

  if (entry.tags.length > 0) {
    height += 6;
  }

  return Math.max(height, 16) * 4;
};

export const EntryItem = ({
  entry,
  selected = false,
  className,
  variant,
  tags: tagCatalog = [],
  style,
  ...props
}: ComponentProps<typeof Item> & {
  entry: EntrySummary;
  selected?: boolean;
  tags?: TagSummary[];
}) => {
  const title = getEntryTitle(entry);
  const username = entry.username || (entry.usernameIsProtected ? 'Protected username' : undefined);
  const url = entry.url || (entry.urlIsProtected ? 'Protected URL' : undefined);
  const description = joinComponent(
    [username, url].filter(Boolean),
    <span className="inline-block align-[-0.1cap] w-0.5 h-[1.2cap] mx-2 rounded-full bg-[currentColor] opacity-30 rotate-30" />,
  );

  return (
    <Item
      variant={variant ?? (selected ? 'muted' : 'default')}
      className={cx('flex-nowrap', selected && 'bg-primary', className)}
      style={{ ...style, height: `${getEntryItemSize(entry)}px` }}
      {...props}
    >
      <ItemMedia variant="icon" className={cx(selected && 'text-primary-foreground')}>
        <ItemIcon icon={entry.icon} fallback="entry" />
      </ItemMedia>
      <ItemContent className="min-w-0 gap-0.5">
        <ItemTitle className={cx(selected && 'text-primary-foreground')}>{title}</ItemTitle>
        <ItemDescription className="flex flex-col gap-0.5 min-h-5.5">
          {!!description.length && (
            <div className="flex h-5.5 items-center gap-1 overflow-hidden">
              <span
                className={cx(
                  'min-w-0 truncate break-all',
                  selected && 'text-primary-foreground/75',
                )}
              >
                {description}
              </span>
            </div>
          )}
          {!!entry.tags.length && (
            <div className="flex h-5.5 mt-1 items-center gap-1 overflow-hidden">
              {entry.tags.map(name => (
                <Tag
                  key={name}
                  name={name}
                  tagStyle={tagCatalog.find(tag => tag.name === name)?.style}
                  variant={selected ? 'transparent' : 'default'}
                  className="max-w-24"
                />
              ))}
            </div>
          )}
        </ItemDescription>
      </ItemContent>
    </Item>
  );
};
