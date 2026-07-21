import {
  Command,
  CommandDialog,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from '@/components/command';
import { useRequest } from '@/fragments/_providers/QueryProvider';
import { useNavigate } from '@/fragments/_providers/RouterProvider';
import { IconSearch, IconTag, IconTrash } from '@/icons';
import { buildRoute } from '@/utils/route';
import fuzzysort from 'fuzzysort';
import { useEffect, useState } from 'react';
import { getEntryTitle } from './EntryItem';
import { flattenHierarchy } from './GroupTree';
import { ItemIcon } from './ItemIcon';
import type { ReactNode } from 'react';

const RESULT_LIMIT = 8;

type SearchRecord<T> = {
  item: T;
  text: string;
};

const fuzzySearch = <T,>(query: string, records: SearchRecord<T>[]) =>
  fuzzysort
    .go(query.normalize('NFKD'), records, {
      key: record => record.text.normalize('NFKD'),
      limit: RESULT_LIMIT,
    })
    .map(result => result.obj.item);

const CommandResult = ({
  icon,
  label,
  description,
  shortcut,
}: {
  icon: ReactNode;
  label: string;
  description?: string;
  shortcut?: ReactNode;
}) => (
  <>
    {icon}
    <div className="min-w-0 flex-1">
      <div className="truncate">{label}</div>
      {description && <div className="truncate text-xs text-muted-foreground">{description}</div>}
    </div>
    {shortcut && <CommandShortcut>{shortcut}</CommandShortcut>}
  </>
);

const SearchCommandResults = ({
  onOpenChange,
  onSearch,
}: {
  onOpenChange: (open: boolean) => void;
  onSearch: (query: string) => void;
}) => {
  const [query, setQuery] = useState('');
  const entries = useRequest('getEntries', { excludeTrash: true });
  const hierarchy = useRequest('getGroupHierarchy', {});
  const tags = useRequest('getTags', {});
  const navigate = useNavigate();
  const trimmedQuery = query.trim();
  const entryResults = fuzzySearch(
    trimmedQuery,
    (entries.data?.entries ?? []).map(entry => ({
      item: entry,
      text: [getEntryTitle(entry), entry.username, entry.url, ...entry.tags]
        .filter(Boolean)
        .join(' '),
    })),
  );
  const groupResults = fuzzySearch(
    trimmedQuery,
    (hierarchy.data ? flattenHierarchy(hierarchy.data) : []).map(({ group }) => ({
      item: group,
      text: group.name,
    })),
  );
  const tagResults = fuzzySearch(
    trimmedQuery,
    (tags.data?.tags ?? []).map(tag => ({ item: tag, text: tag.name })),
  );
  const trashMatches = Boolean(trimmedQuery && fuzzysort.single(trimmedQuery, 'Trash Recycle Bin'));
  const loading = entries.isPending || hierarchy.isPending || tags.isPending;

  const select = (href: string) => {
    onOpenChange(false);
    navigate(href);
  };

  return (
    <Command shouldFilter={false} loop>
      <CommandInput
        value={query}
        onValueChange={setQuery}
        maxLength={256}
        placeholder="Search entries, groups, and tags..."
      />
      <CommandList>
        {!trimmedQuery && (
          <p className="px-3 py-8 text-center text-sm text-muted-foreground">
            Type to search entries, groups, tags, and Trash.
          </p>
        )}
        {entryResults.length > 0 && (
          <CommandGroup heading="Entries">
            {entryResults.map(entry => (
              <CommandItem
                key={String(entry.id)}
                value={`entry:${String(entry.id)}`}
                onSelect={() =>
                  select(`${buildRoute('database')}?entry=${encodeURIComponent(String(entry.id))}`)
                }
              >
                <CommandResult
                  icon={<ItemIcon icon={entry.icon} fallback="entry" />}
                  label={getEntryTitle(entry)}
                  description={[entry.username, entry.url].filter(Boolean).join(' | ')}
                />
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        {trimmedQuery && (
          <CommandGroup heading="Search">
            <CommandItem
              value={`search:${trimmedQuery}`}
              onSelect={() => {
                onOpenChange(false);
                onSearch(trimmedQuery);
              }}
            >
              <CommandResult
                icon={<IconSearch />}
                label={`Search for "${trimmedQuery}"`}
                shortcut="Enter"
              />
            </CommandItem>
          </CommandGroup>
        )}
        {groupResults.length > 0 && (
          <CommandGroup heading="Groups">
            {groupResults.map(group => (
              <CommandItem
                key={String(group.id)}
                value={`group:${String(group.id)}`}
                onSelect={() => select(buildRoute('group', { group: String(group.id) }))}
              >
                <CommandResult
                  icon={<ItemIcon icon={group.icon} fallback="group" />}
                  label={group.name || 'Untitled group'}
                />
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        {tagResults.length > 0 && (
          <CommandGroup heading="Tags">
            {tagResults.map(tag => (
              <CommandItem
                key={tag.name}
                value={`tag:${tag.name}`}
                onSelect={() => select(buildRoute('tag', { tag: tag.name }))}
              >
                <CommandResult
                  icon={<IconTag />}
                  label={tag.name}
                  description={`${tag.entryCount} ${tag.entryCount === 1 ? 'entry' : 'entries'}`}
                />
              </CommandItem>
            ))}
          </CommandGroup>
        )}
        {trashMatches && (
          <CommandGroup heading="Navigation">
            <CommandItem value="navigation:trash" onSelect={() => select(buildRoute('trash'))}>
              <CommandResult icon={<IconTrash />} label="Trash" />
            </CommandItem>
          </CommandGroup>
        )}
        {trimmedQuery &&
          !loading &&
          entryResults.length === 0 &&
          groupResults.length === 0 &&
          tagResults.length === 0 &&
          !trashMatches && (
            <p className="px-3 pb-3 text-center text-xs text-muted-foreground">
              No quick results. Press Enter to search all entry fields.
            </p>
          )}
        {loading && trimmedQuery && (
          <p className="px-3 pb-3 text-center text-xs text-muted-foreground">
            Loading quick results...
          </p>
        )}
      </CommandList>
    </Command>
  );
};

const isEditableTarget = (target: EventTarget | null) =>
  target instanceof HTMLElement &&
  (target.matches('input, textarea, select') || target.isContentEditable);

export const SearchCommand = ({
  open,
  onOpenChange,
  onSearch,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSearch: (query: string) => void;
}) => {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (
        event.ctrlKey &&
        event.key.toLowerCase() === 'p' &&
        !event.altKey &&
        !event.metaKey &&
        !isEditableTarget(event.target)
      ) {
        event.preventDefault();
        onOpenChange(true);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onOpenChange]);

  return (
    <CommandDialog open={open} onOpenChange={onOpenChange} className="sm:max-w-xl">
      {open && <SearchCommandResults onOpenChange={onOpenChange} onSearch={onSearch} />}
    </CommandDialog>
  );
};
