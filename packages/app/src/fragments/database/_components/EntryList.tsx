import { Alert, AlertDescription, AlertTitle } from '@/components/alert';
import { Skeleton } from '@/components/skeleton';
import { useRequest, useRequestClient } from '@/fragments/_providers/QueryProvider';
import { useHistoryBack } from '@/fragments/_providers/RouterProvider';
import { useDebouncedValue } from '@/hooks/useDebouncedValue';
import { useIsMobile } from '@/hooks/useIsMobile';
import { IconAlertCircle } from '@/icons';
import { cx } from '@/utils/css';
import { buildRoute, getRoute } from '@/utils/route';
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
import { EntryListHeader } from './EntryListHeader';
import { searchFilterToken } from './searchQuery';
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
      className={cx(
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
      className={cx(
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
  searchQuery,
  onOpenSearch,
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
  searchQuery?: string;
  onOpenSearch?: (initialQuery: string) => void;
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
      queryClient.invalidateQueries({ queryKey: ['request', 'searchFuzzy'] }),
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
        className={cx(
          'min-h-0 w-full flex-2 flex-col border-r md:flex md:max-w-sm xl:max-w-md',
          selectedEntry ? 'hidden' : 'flex',
        )}
      >
        <div className="flex flex-col xl:px-6">
          <EntryListHeader
            title={title}
            result={result}
            creationParentId={creationParentId}
            movePending={movePending}
            addPending={addPending}
            templates={templates}
            onAdd={() => creationParentId && addEntry.mutate({ parentGroupId: creationParentId })}
            onAddFromTemplate={templateEntryId =>
              creationParentId &&
              addEntryFromTemplate.mutate({ parentGroupId: creationParentId, templateEntryId })
            }
            searchQuery={searchQuery}
            onOpenSearch={onOpenSearch}
          />

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
  onOpenSearch,
}: {
  groupParam: string;
  movePending: boolean;
  moveError: boolean;
  hiddenEntry?: HiddenEntry;
  onOpenSearch: (initialQuery: string) => void;
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
      searchQuery={`${searchFilterToken('in', group.name)} `}
      onOpenSearch={onOpenSearch}
    />
  );
};

export const EntryList = ({
  movePending,
  moveError,
  hiddenEntry,
  searchQueries,
  onOpenSearch,
}: {
  movePending: boolean;
  moveError: boolean;
  hiddenEntry?: HiddenEntry;
  searchQueries: Record<string, string>;
  onOpenSearch: (initialQuery: string) => void;
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
        onOpenSearch={onOpenSearch}
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
        onOpenSearch={onOpenSearch}
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
        searchQuery={`${searchFilterToken('tag', tag)} `}
        onOpenSearch={onOpenSearch}
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
        onOpenSearch={onOpenSearch}
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
      onOpenSearch={onOpenSearch}
    />
  );
};
