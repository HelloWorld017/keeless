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
import { useDebouncedValue } from '@/hooks/useDebouncedValue';
import { IconSearch, IconTag, IconTrash } from '@/icons';
import { buildRoute } from '@/utils/route';
import { useEffect, useState } from 'react';
import { getEntryTitle } from './EntryItem';
import { ItemIcon } from './ItemIcon';
import { searchFilterToken } from './searchQuery';
import type { ReactNode } from 'react';

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
  initialQuery,
  onOpenChange,
  onSearch,
}: {
  initialQuery: string;
  onOpenChange: (open: boolean) => void;
  onSearch: (query: string) => void;
}) => {
  const [query, setQuery] = useState(initialQuery);
  const debouncedQuery = useDebouncedValue(query, 150, initialQuery);
  const search = useRequest('searchFuzzy', { query: debouncedQuery });
  const navigate = useNavigate();
  const isCurrentQuery = query === debouncedQuery;
  const loading = !isCurrentQuery || search.isPending || search.isFetching;
  const result = isCurrentQuery && !loading ? search.data : undefined;
  const trimmedQuery = query.trim();

  const select = (href: string) => {
    onOpenChange(false);
    navigate(href);
  };
  const complete = (prefix: 'in' | 'tag', name: string) => {
    const tokens = result?.filterTokens ?? [];
    setQuery([...tokens, searchFilterToken(prefix, name)].join(' '));
  };

  return (
    <Command
      shouldFilter={false}
      loop
      onKeyDown={event => {
        if (event.key !== 'Tab') {
          return;
        }
        const selected = event.currentTarget.querySelector<HTMLElement>(
          '[cmdk-item][data-selected="true"][data-completion]',
        );
        const completion = selected?.dataset.completion;
        if (!completion) {
          return;
        }
        const [prefix, ...name] = completion.split(':');
        if ((prefix === 'in' || prefix === 'tag') && name.length > 0) {
          event.preventDefault();
          complete(prefix, name.join(':'));
        }
      }}
    >
      <CommandInput
        value={query}
        onValueChange={setQuery}
        maxLength={256}
        placeholder="Search entries, groups, and tags..."
      />
      <CommandList>
        {!trimmedQuery && !loading && (
          <p className="px-3 py-8 text-center text-sm text-muted-foreground">
            Type to search entries, groups, tags, and Trash.
          </p>
        )}
        {trimmedQuery && result?.entries.length ? (
          <CommandGroup heading="Entries">
            {result.entries.map(entry => (
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
        ) : null}
        {trimmedQuery && (
          <CommandGroup heading="Search">
            <CommandItem
              value={`search:${trimmedQuery}`}
              onSelect={() => {
                onOpenChange(false);
                onSearch(query);
              }}
            >
              <CommandResult
                icon={<IconSearch />}
                label={`Search for "${query}"`}
                shortcut="Enter"
              />
            </CommandItem>
          </CommandGroup>
        )}
        {trimmedQuery && result?.groups.length ? (
          <CommandGroup heading="Groups">
            {result.groups.map(group => (
              <CommandItem
                key={String(group.id)}
                value={`group:${String(group.id)}`}
                data-completion={`in:${group.name}`}
                onSelect={() => select(buildRoute('group', { group: String(group.id) }))}
              >
                <CommandResult
                  icon={<ItemIcon icon={group.icon} fallback="group" />}
                  label={group.name || 'Untitled group'}
                  shortcut="Tab"
                />
              </CommandItem>
            ))}
          </CommandGroup>
        ) : null}
        {trimmedQuery && result?.tags.length ? (
          <CommandGroup heading="Tags">
            {result.tags.map(tag => (
              <CommandItem
                key={tag.name}
                value={`tag:${tag.name}`}
                data-completion={`tag:${tag.name}`}
                onSelect={() => select(buildRoute('tag', { tag: tag.name }))}
              >
                <CommandResult
                  icon={<IconTag />}
                  label={tag.name}
                  description={`${tag.entryCount} ${tag.entryCount === 1 ? 'entry' : 'entries'}`}
                  shortcut="Tab"
                />
              </CommandItem>
            ))}
          </CommandGroup>
        ) : null}
        {trimmedQuery && result?.trashMatches && (
          <CommandGroup heading="Navigation">
            <CommandItem value="navigation:trash" onSelect={() => select(buildRoute('trash'))}>
              <CommandResult icon={<IconTrash />} label="Trash" />
            </CommandItem>
          </CommandGroup>
        )}
        {loading && (
          <p className="px-3 pb-3 text-center text-xs text-muted-foreground">
            Loading quick results...
          </p>
        )}
        {search.isError && isCurrentQuery && !loading && (
          <p className="px-3 pb-3 text-center text-xs text-destructive">
            Search could not be loaded.
          </p>
        )}
        {trimmedQuery &&
          result &&
          result.entries.length === 0 &&
          result.groups.length === 0 &&
          result.tags.length === 0 &&
          !result.trashMatches && (
            <p className="px-3 pb-3 text-center text-xs text-muted-foreground">
              No quick results. Press Enter to search all entries.
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
  initialQuery,
  onOpenChange,
  onSearch,
}: {
  open: boolean;
  initialQuery: string;
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
      {open && (
        <SearchCommandResults
          initialQuery={initialQuery}
          onOpenChange={onOpenChange}
          onSearch={onSearch}
        />
      )}
    </CommandDialog>
  );
};
