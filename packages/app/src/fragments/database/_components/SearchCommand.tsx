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
import { useThrottledValue } from '@/hooks/useDebouncedValue';
import { useLatestRef } from '@/hooks/useLatestRef';
import { IconSearch, IconTag, IconTrash } from '@/icons';
import { buildRoute } from '@/utils/route';
import { useCallback, useEffect, useLayoutEffect, useMemo, useState } from 'react';
import { getEntryTitle } from './EntryItem';
import { ItemIcon } from './ItemIcon';
import { searchFilterToken } from '../_utils/searchQuery';
import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from 'react';

type CommandItemType = {
  value: string;
  group: 'entry' | 'group' | 'search' | 'tag' | 'navigation';
  completion?: string;
  onSelect: () => void;
  icon: ReactNode;
  label: string;
  description?: string;
  shortcut?: ReactNode;
};

const GROUP_HEADINGS = {
  entry: 'Entries',
  group: 'Groups',
  search: 'Search',
  tag: 'Tags',
  navigation: 'Navigation',
} satisfies Record<CommandItemType['group'], string>;

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
  const debouncedQuery = useThrottledValue(query, 150, initialQuery);
  const search = useRequest(
    'searchFuzzy',
    { query: debouncedQuery },
    {
      enabled: !!debouncedQuery.trim(),
      gcTime: 30 * 1000,
      placeholderData: previousData => previousData,
    },
  );
  const navigate = useNavigate();
  const isCurrentQuery = query === debouncedQuery;
  const loading = !isCurrentQuery || search.isPending || search.isFetching;
  const result = search.data;
  const trimmedQuery = query.trim();

  const onOpenChangeRef = useLatestRef(onOpenChange);
  const onSearchRef = useLatestRef(onSearch);

  const select = useCallback(
    (href: string) => {
      onOpenChangeRef.current(false);
      navigate(href);
    },
    [onOpenChangeRef, navigate],
  );

  const complete = (prefix: 'in' | 'tag', name: string) => {
    const tokens = result?.filterTokens ?? [];
    setQuery([...tokens, searchFilterToken(prefix, name)].join(' ') + ' ');
  };

  const items = useMemo<CommandItemType[]>(
    () => [
      ...(result?.entries.map(entry => ({
        value: `entry:${String(entry.id)}`,
        group: 'entry' as const,
        onSelect: () =>
          select(`${buildRoute('database')}?entry=${encodeURIComponent(String(entry.id))}`),
        icon: <ItemIcon icon={entry.icon} fallback="entry" />,
        label: getEntryTitle(entry),
        description: [entry.username, entry.url].filter(Boolean).join(' | '),
      })) ?? []),
      {
        value: `search:${trimmedQuery}`,
        group: 'search' as const,
        onSelect: () => {
          onOpenChangeRef.current(false);
          onSearchRef.current(trimmedQuery);
        },
        icon: <IconSearch />,
        label: `Search for "${trimmedQuery}"`,
        shortcut: 'Enter',
      },
      ...(result?.groups.map(group => ({
        value: `group:${String(group.id)}`,
        group: 'group' as const,
        completion: `in:${group.name}`,
        onSelect: () => select(buildRoute('group', { group: String(group.id) })),
        icon: <ItemIcon icon={group.icon} fallback="group" />,
        label: group.name || 'Untitled group',
        shortcut: 'Tab',
      })) ?? []),
      ...(result?.tags.map(tag => ({
        value: `tag:${tag.name}`,
        group: 'tag' as const,
        completion: `tag:${tag.name}`,
        onSelect: () => select(buildRoute('tag', { tag: tag.name })),
        icon: <IconTag />,
        label: tag.name,
        description: `${tag.entryCount} ${tag.entryCount === 1 ? 'entry' : 'entries'}`,
        shortcut: 'Tab',
      })) ?? []),
      ...(result?.trashMatches
        ? [
            {
              value: 'navigation:trash',
              group: 'navigation' as const,
              onSelect: () => select(buildRoute('trash')),
              icon: <IconTrash />,
              label: 'Trash',
            },
          ]
        : []),
    ],
    [trimmedQuery, result, select, onSearchRef, onOpenChangeRef],
  );

  const itemsRendered = items
    .reduce<{ key: CommandItemType['group']; children: CommandItemType[] }[]>((groups, item) => {
      const lastGroup = groups.at(-1);
      if (lastGroup?.key === item.group) {
        lastGroup.children.push(item);
        return groups;
      }

      return [...groups, { key: item.group, children: [item] }];
    }, [])
    .map(({ key, children }) => (
      <CommandGroup key={key} heading={GROUP_HEADINGS[key]}>
        {children.map(item => (
          <CommandItem key={item.value} value={item.value} onSelect={item.onSelect}>
            <CommandResult
              icon={item.icon}
              label={item.label}
              description={item.description}
              shortcut={item.shortcut}
            />
          </CommandItem>
        ))}
      </CommandGroup>
    ));

  const [value, setValue] = useState<string | undefined>(undefined);
  useLayoutEffect(() => {
    setValue(items?.[0].value);
  }, [query, items]);

  const onKeyDown = (event: ReactKeyboardEvent) => {
    if (event.key !== 'Tab') {
      return;
    }

    const selected = items.find(item => item.value === value);
    const completion = selected?.completion;
    if (!completion) {
      return;
    }

    const [prefix, ...name] = completion.split(':');
    if ((prefix === 'in' || prefix === 'tag') && name.length > 0) {
      event.preventDefault();
      complete(prefix, name.join(':'));
    }
  };

  return (
    <Command shouldFilter={false} value={value} onValueChange={setValue} onKeyDown={onKeyDown} loop>
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
        {trimmedQuery && itemsRendered}
        {trimmedQuery && !result && (
          <p className="px-3 pb-3 text-center text-xs text-muted-foreground">
            Loading quick results...
          </p>
        )}
        {search.isError && isCurrentQuery && !loading && (
          <p className="px-3 pb-3 text-center text-xs text-destructive">
            Search could not be loaded.
          </p>
        )}
        {trimmedQuery && !items.filter(item => item.group !== 'search').length && (
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
  const onOpenChangeRef = useLatestRef(onOpenChange);
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
        onOpenChangeRef.current(true);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onOpenChangeRef]);

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
