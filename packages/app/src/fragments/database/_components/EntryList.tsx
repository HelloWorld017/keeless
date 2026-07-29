import { Alert, AlertDescription, AlertTitle } from '@/components/alert';
import { Button } from '@/components/button';
import { Skeleton } from '@/components/skeleton';
import { useRequest, useRequestClient } from '@/fragments/_providers/QueryProvider';
import { useHistoryBack } from '@/fragments/_providers/RouterProvider';
import { useIsMobile } from '@/hooks/use-mobile';
import { useDebouncedValue } from '@/hooks/useDebouncedValue';
import { IconAlertCircle, IconChevronDown, IconLoaderCircle, IconPlus } from '@/icons';
import { cn, cx } from '@/utils/css';
import { buildRoute, getRoute } from '@/utils/route';
import { Menu } from '@base-ui/react/menu';
import { useDraggable } from '@dnd-kit/core';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useMemo, useRef } from 'react';
import { Redirect, useRoute, useSearchParams } from 'wouter';
import {
  databaseNodeKey,
  entryDndId,
  type EntryDragData,
  type EntryDragSource,
} from '../_utils/dragAndDrop';
import { EntryDetailFragment } from '../entryDetail/EntryDetailFragment';
import { EntryItem, getEntryItemSize, getEntryTitle } from './EntryItem';
import { ItemIcon } from './ItemIcon';
import type { OperationArgs, OperationName } from '@/utils/request';
import type {
  AddEntryArgs,
  AddEntryFromTemplateArgs,
  DatabaseNodeId,
  EntriesResult,
  EntrySummary,
  TagSummary,
} from '@keeless/schema';

type EntryOperationName = Extract<
  OperationName,
  'getEntries' | 'searchEntries' | 'getGroupEntries' | 'getTagEntries' | 'getTrashEntries'
>;

type HiddenEntry = Pick<EntryDragData, 'entryId' | 'source'>;

const isSameSource = (left: EntryDragSource, right: EntryDragSource) =>
  left.type === right.type &&
  (left.type !== 'group' ||
    (right.type === 'group' && databaseNodeKey(left.groupId) === databaseNodeKey(right.groupId)));

const decodeRouteParam = (value: string) => {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
};

type EntryRowProps = {
  entry: EntrySummary;
  tags: TagSummary[];
  source: EntryDragSource;
  selectedEntry: string | null;
  disabled: boolean;
  hiddenEntryId?: DatabaseNodeId;
  onSelect: (entry: EntrySummary) => void;
};

const VirtualEntryRow = ({
  entry,
  tags,
  source,
  selectedEntry,
  disabled,
  hiddenEntryId,
  onSelect,
}: EntryRowProps) => {
  const title = getEntryTitle(entry);
  const data: EntryDragData = { type: 'entry', entryId: entry.id, title, entry, source };
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: entryDndId(entry.id),
    data,
    disabled,
  });

  return (
    <EntryItem
      ref={setNodeRef}
      entry={entry}
      tags={tags}
      selected={selectedEntry === String(entry.id)}
      render={<button type="button" aria-label={title} />}
      className={cn(
        'transition-opacity',
        isDragging ? 'z-10 cursor-grabbing opacity-60' : 'cursor-pointer',
        hiddenEntryId !== undefined &&
          databaseNodeKey(hiddenEntryId) === databaseNodeKey(entry.id) &&
          'opacity-0',
      )}
      onClick={() => onSelect(entry)}
      {...attributes}
      {...listeners}
      aria-current={selectedEntry === String(entry.id)}
    />
  );
};

const VirtualEntryList = ({
  className,
  entries,
  tags,
  source,
  selectedEntry,
  disabled,
  hiddenEntryId,
  onSelect,
}: Omit<EntryRowProps, 'entry'> & { entries: EntrySummary[]; className?: string }) => {
  const scrollRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: index => getEntryItemSize(entries[index]) + 8,
    getItemKey: index => entryDndId(entries[index].id),
    overscan: 4,
  });

  return (
    <div ref={scrollRef} className={cx('min-h-0 w-full flex-1 overflow-auto py-2', className)}>
      <ul className="relative w-full list-none" style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map(virtualRow => {
          const entry = entries[virtualRow.index];

          return (
            <li
              key={virtualRow.key}
              className="absolute left-0 top-0 w-full px-3 py-0.5"
              style={{
                height: virtualRow.size,
                transform: `translateY(${virtualRow.start}px)`,
              }}
              aria-posinset={virtualRow.index + 1}
              aria-setsize={entries.length}
            >
              <VirtualEntryRow
                entry={entry}
                tags={tags}
                source={source}
                selectedEntry={selectedEntry}
                disabled={disabled}
                hiddenEntryId={hiddenEntryId}
                onSelect={onSelect}
              />
            </li>
          );
        })}
      </ul>
    </div>
  );
};

const EntryListSkeleton = ({ pending }: { pending: boolean }) => {
  const visible = useDebouncedValue(pending, 150, false);

  if (!visible) {
    return null;
  }

  return (
    <div
      className={cn(
        'animate-in space-y-2 p-3 duration-200 fade-in',
        !pending && 'animate-out opacity-0 fade-out',
      )}
      aria-label="Loading entries"
    >
      {Array.from({ length: 6 }, (_, index) => (
        <div key={index} className="flex items-center gap-3 px-3 py-2.5">
          <Skeleton className="size-4 shrink-0" />
          <div className="flex-1 space-y-2">
            <Skeleton className="h-4 w-2/5" />
            <Skeleton className="h-3 w-3/5" />
          </div>
        </div>
      ))}
    </div>
  );
};

const EntryQuery = <TName extends EntryOperationName>({
  name,
  args,
  source,
  title,
  movePending,
  moveError,
  hiddenEntry,
  creationParentId,
  preserveOrder = false,
}: {
  name: TName;
  args: OperationArgs<TName>;
  source: EntryDragSource;
  title: string;
  movePending: boolean;
  moveError: boolean;
  hiddenEntry?: HiddenEntry;
  creationParentId?: DatabaseNodeId;
  preserveOrder?: boolean;
}) => {
  const entries = useRequest(name, args);
  const templates = useRequest('getEntryTemplates', {});
  const tags = useRequest('getTags', {});
  const requestClient = useRequestClient();
  const queryClient = useQueryClient();
  const [searchParams, setSearchParams] = useSearchParams();
  const isMobile = useIsMobile();
  const historyBack = useHistoryBack();
  const selectedEntry = searchParams.get('entry');
  const onAddSuccess = async (result: { id: DatabaseNodeId }) => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ['request', 'getEntries'] }),
      queryClient.invalidateQueries({ queryKey: ['request', 'searchEntries'] }),
      queryClient.invalidateQueries({ queryKey: ['request', 'getGroupEntries'] }),
      queryClient.invalidateQueries({ queryKey: ['request', 'getEntryTemplates'] }),
      queryClient.invalidateQueries({ queryKey: ['request', 'getTagEntries'] }),
      queryClient.invalidateQueries({ queryKey: ['request', 'getTags'] }),
    ]);
    setSearchParams({ entry: String(result.id) }, { replace: !isMobile });
  };
  const addEntry = useMutation({
    mutationFn: (addArgs: AddEntryArgs) => requestClient.data!.request('addEntry', addArgs),
    onSuccess: onAddSuccess,
  });
  const addEntryFromTemplate = useMutation({
    mutationFn: (addArgs: AddEntryFromTemplateArgs) =>
      requestClient.data!.request('addEntryFromTemplate', addArgs),
    onSuccess: onAddSuccess,
  });
  const addPending = addEntry.isPending || addEntryFromTemplate.isPending;
  const result = entries.data as EntriesResult | undefined;
  const resultSorted = useMemo(() => {
    if (!result) {
      return [];
    }
    if (preserveOrder) {
      return result.entries;
    }
    return result.entries.toSorted((a, b) => {
      if (!a.name) {
        return -1;
      }

      if (!b.name) {
        return -1;
      }

      return a.name.localeCompare(b.name);
    });
  }, [preserveOrder, result]);
  const selectedEntrySummary = resultSorted.find(entry => String(entry.id) === selectedEntry);
  const closeEntry = () => {
    if (isMobile) {
      historyBack();
      return;
    }

    setSearchParams({}, { replace: true });
  };

  return (
    <>
      <section
        className={cn(
          'min-h-0 w-full flex-2 flex-col border-r md:flex md:max-w-sm xl:max-w-md',
          selectedEntry ? 'hidden' : 'flex',
        )}
      >
        <div className="flex flex-col xl:px-6">
          <header className="flex min-h-16 items-start justify-between gap-3 px-4 py-3 xl:py-6 xl:pb-4">
            <div className="min-w-0">
              <h1 className="truncate text-xl font-semibold">{title}</h1>
              {result && (
                <span className="shrink-0 text-sm tabular-nums text-muted-foreground">
                  {result.entries.length} {result.entries.length === 1 ? 'Entry' : 'Entries'}
                </span>
              )}
            </div>
            {creationParentId !== undefined && (
              <div className="flex shrink-0">
                <Button
                  type="button"
                  variant="outline"
                  size="icon"
                  className="rounded-r-none"
                  aria-label="Add entry"
                  disabled={movePending || addPending}
                  onClick={() => addEntry.mutate({ parentGroupId: creationParentId })}
                >
                  {addPending ? <IconLoaderCircle className="animate-spin" /> : <IconPlus />}
                </Button>
                <Menu.Root>
                  <Menu.Trigger
                    render={
                      <Button
                        type="button"
                        variant="outline"
                        size="icon"
                        className="-ml-px rounded-l-none"
                        aria-label="Add entry from template"
                      />
                    }
                    disabled={movePending || addPending}
                  >
                    <IconChevronDown />
                  </Menu.Trigger>
                  <Menu.Portal>
                    <Menu.Positioner align="end" sideOffset={4} className="isolate z-50">
                      <Menu.Popup className="max-h-(--available-height) min-w-52 origin-(--transform-origin) overflow-y-auto rounded-lg bg-popover/90 p-1 text-popover-foreground shadow-md ring-1 ring-foreground/10 backdrop-blur-xl duration-100 data-[side=bottom]:slide-in-from-top-2 data-[side=top]:slide-in-from-bottom-2 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95">
                        {templates.isPending && (
                          <Menu.Item
                            disabled
                            className="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-muted-foreground outline-none"
                          >
                            <IconLoaderCircle className="animate-spin" />
                            Loading templates
                          </Menu.Item>
                        )}
                        {templates.isError && (
                          <Menu.Item
                            disabled
                            className="rounded-md px-2 py-1.5 text-sm text-destructive outline-none"
                          >
                            Templates could not be loaded
                          </Menu.Item>
                        )}
                        {templates.data?.entries.length === 0 && (
                          <Menu.Item
                            disabled
                            className="rounded-md px-2 py-1.5 text-sm text-muted-foreground outline-none"
                          >
                            No templates
                          </Menu.Item>
                        )}
                        {templates.data?.entries.map(template => (
                          <Menu.Item
                            key={String(template.id)}
                            className="flex cursor-default items-center gap-2 rounded-md px-2 py-1.5 text-sm outline-none data-highlighted:bg-foreground/10"
                            onClick={() =>
                              addEntryFromTemplate.mutate({
                                parentGroupId: creationParentId,
                                templateEntryId: template.id,
                              })
                            }
                          >
                            <ItemIcon
                              icon={template.icon}
                              fallback="entry"
                              className="size-4 shrink-0 text-muted-foreground"
                            />
                            <span className="min-w-0 truncate">{getEntryTitle(template)}</span>
                          </Menu.Item>
                        ))}
                      </Menu.Popup>
                    </Menu.Positioner>
                  </Menu.Portal>
                </Menu.Root>
              </div>
            )}
          </header>

          {moveError && (
            <Alert variant="destructive" className="m-3 mb-0 w-auto">
              <IconAlertCircle />
              <AlertTitle>Entry could not be moved</AlertTitle>
              <AlertDescription>Try moving the entry again.</AlertDescription>
            </Alert>
          )}
          {(addEntry.isError || addEntryFromTemplate.isError) && (
            <Alert variant="destructive" className="m-3 mb-0 w-auto">
              <IconAlertCircle />
              <AlertTitle>Entry could not be added</AlertTitle>
              <AlertDescription>Try adding the entry again.</AlertDescription>
            </Alert>
          )}

          <EntryListSkeleton pending={entries.isPending} />
          {entries.isError && (
            <Alert variant="destructive" className="m-3 w-auto">
              <IconAlertCircle />
              <AlertTitle>Entries could not be loaded</AlertTitle>
              <AlertDescription>Try opening this section again.</AlertDescription>
            </Alert>
          )}
          {result && result.entries.length === 0 && (
            <p className="p-6 text-center text-sm text-muted-foreground">No entries</p>
          )}
        </div>

        {result && result.entries.length > 0 && (
          <VirtualEntryList
            className="xl:px-6"
            entries={resultSorted}
            tags={tags.data?.tags ?? []}
            source={source}
            selectedEntry={selectedEntry}
            disabled={movePending}
            hiddenEntryId={
              hiddenEntry && isSameSource(source, hiddenEntry.source)
                ? hiddenEntry.entryId
                : undefined
            }
            onSelect={entry => setSearchParams({ entry: String(entry.id) }, { replace: !isMobile })}
          />
        )}
      </section>
      <EntryDetailFragment
        entry={selectedEntrySummary}
        selected={selectedEntry !== null}
        listPending={entries.isPending}
        inTrash={source.type === 'trash'}
        onClose={closeEntry}
      />
    </>
  );
};

const GroupEntries = ({
  groupParam,
  movePending,
  moveError,
  hiddenEntry,
}: {
  groupParam: string;
  movePending: boolean;
  moveError: boolean;
  hiddenEntry?: HiddenEntry;
}) => {
  const hierarchy = useRequest('getGroupHierarchy', {});

  if (hierarchy.isPending) {
    return (
      <section className="flex min-h-0 w-full flex-2 flex-col border-r md:max-w-md">
        <EntryListSkeleton pending />
      </section>
    );
  }
  if (hierarchy.isError) {
    return (
      <section className="w-full border-r p-3 md:max-w-md">
        <Alert variant="destructive">
          <IconAlertCircle />
          <AlertTitle>Group could not be loaded</AlertTitle>
        </Alert>
      </section>
    );
  }

  const group = hierarchy.data.groups.find(candidate => String(candidate.id) === groupParam);
  if (!group) {
    return (
      <section className="w-full border-r p-6 text-center text-sm text-muted-foreground md:max-w-md">
        Group does not exist
      </section>
    );
  }

  return (
    <EntryQuery
      name="getGroupEntries"
      args={{ groupId: group.id }}
      source={{ type: 'group', groupId: group.id }}
      title={group.name || 'Untitled group'}
      movePending={movePending}
      moveError={moveError}
      hiddenEntry={hiddenEntry}
      creationParentId={group.id}
    />
  );
};

export const EntryList = ({
  movePending,
  moveError,
  hiddenEntry,
  searchQueries,
}: {
  movePending: boolean;
  moveError: boolean;
  hiddenEntry?: HiddenEntry;
  searchQueries: Record<string, string>;
}) => {
  const hierarchy = useRequest('getGroupHierarchy', {});
  const [searchMatch, searchParams] = useRoute<{ search: string }>(getRoute('search'));
  const [groupMatch, groupParams] = useRoute<{ group: string }>(getRoute('group'));
  const [tagMatch, tagParams] = useRoute<{ tag: string }>(getRoute('tag'));
  const [trashMatch] = useRoute(getRoute('trash'));

  if (searchMatch) {
    const searchQuery = searchQueries[searchParams.search];
    if (!searchQuery) {
      return <Redirect to={buildRoute('database')} replace />;
    }
    return (
      <EntryQuery
        name="searchEntries"
        args={{ query: searchQuery }}
        source={{ type: 'search' }}
        title={`Search: ${searchQuery}`}
        movePending={movePending}
        moveError={moveError}
        hiddenEntry={hiddenEntry}
        preserveOrder
      />
    );
  }
  if (groupMatch) {
    return (
      <GroupEntries
        groupParam={groupParams.group}
        movePending={movePending}
        moveError={moveError}
        hiddenEntry={hiddenEntry}
      />
    );
  }
  if (tagMatch) {
    const tag = decodeRouteParam(tagParams.tag);
    return (
      <EntryQuery
        name="getTagEntries"
        args={{ tag }}
        source={{ type: 'tag' }}
        title={tag}
        movePending={movePending}
        moveError={moveError}
        hiddenEntry={hiddenEntry}
      />
    );
  }
  if (trashMatch) {
    return (
      <EntryQuery
        name="getTrashEntries"
        args={{}}
        source={{ type: 'trash' }}
        title="Trash"
        movePending={movePending}
        moveError={moveError}
        hiddenEntry={hiddenEntry}
      />
    );
  }
  return (
    <EntryQuery
      name="getEntries"
      args={{ excludeTrash: true }}
      source={{ type: 'all' }}
      title="All Entries"
      movePending={movePending}
      moveError={moveError}
      hiddenEntry={hiddenEntry}
      creationParentId={hierarchy.data?.rootGroupId}
    />
  );
};
