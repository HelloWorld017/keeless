import { Alert, AlertDescription, AlertTitle } from '@/components/alert';
import { Skeleton } from '@/components/skeleton';
import { useRequest } from '@/fragments/_providers/QueryProvider';
import { useDebouncedValue } from '@/hooks/useDebouncedValue';
import { IconAlertCircle } from '@/icons';
import { cn } from '@/utils/css';
import { getRoute } from '@/utils/route';
import { useDraggable } from '@dnd-kit/core';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useRef } from 'react';
import { useRoute, useSearchParams } from 'wouter';
import { entryDndId, type EntryDragData } from '../_utils/dragAndDrop';
import { EntryItem, getEntryTitle } from './EntryItem';
import type { OperationArgs, OperationName } from '@/utils/request';
import type { EntriesResult, EntrySummary } from '@keeless/schema';

type EntryOperationName = Extract<
  OperationName,
  'getEntries' | 'getGroupEntries' | 'getTagEntries' | 'getTrashEntries'
>;

const decodeRouteParam = (value: string) => {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
};

type EntryRowProps = {
  entry: EntrySummary;
  selectedEntry: string | null;
  disabled: boolean;
  onSelect: (entry: EntrySummary) => void;
};

const VirtualEntryRow = ({ entry, selectedEntry, disabled, onSelect }: EntryRowProps) => {
  const title = getEntryTitle(entry);
  const data: EntryDragData = { type: 'entry', entryId: entry.id, title, entry };
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: entryDndId(entry.id),
    data,
    disabled,
  });

  return (
    <EntryItem
      ref={setNodeRef}
      entry={entry}
      selected={selectedEntry === String(entry.id)}
      render={<button type="button" aria-label={title} />}
      className={isDragging ? 'z-10 cursor-grabbing opacity-60' : 'cursor-pointer'}
      onClick={() => onSelect(entry)}
      {...attributes}
      {...listeners}
      aria-pressed={selectedEntry === String(entry.id)}
    />
  );
};

const VirtualEntryList = ({
  entries,
  selectedEntry,
  disabled,
  onSelect,
}: Omit<EntryRowProps, 'entry'> & { entries: EntrySummary[] }) => {
  const scrollRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 68,
    getItemKey: index => entryDndId(entries[index].id),
    overscan: 4,
  });

  return (
    <div ref={scrollRef} className="min-h-0 w-full flex-1 overflow-auto py-2">
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
                selectedEntry={selectedEntry}
                disabled={disabled}
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
  title,
  movePending,
  moveError,
}: {
  name: TName;
  args: OperationArgs<TName>;
  title: string;
  movePending: boolean;
  moveError: boolean;
}) => {
  const entries = useRequest(name, args);
  const [searchParams, setSearchParams] = useSearchParams();
  const selectedEntry = searchParams.get('entry');
  const result = entries.data as EntriesResult | undefined;

  return (
    <section className="flex min-h-0 w-full flex-1 flex-col border-r md:max-w-md">
      <header className="flex min-h-14 items-center justify-between gap-4 border-b px-4 py-3">
        <h1 className="truncate text-base font-semibold">{title}</h1>
        {result && (
          <span className="shrink-0 text-sm tabular-nums text-muted-foreground">
            {result.entries.length}
          </span>
        )}
      </header>

      {moveError && (
        <Alert variant="destructive" className="m-3 mb-0 w-auto">
          <IconAlertCircle />
          <AlertTitle>Entry could not be moved</AlertTitle>
          <AlertDescription>Try moving the entry again.</AlertDescription>
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
      {result && result.entries.length > 0 && (
        <VirtualEntryList
          entries={result.entries}
          selectedEntry={selectedEntry}
          disabled={movePending}
          onSelect={entry => setSearchParams({ entry: String(entry.id) }, { replace: true })}
        />
      )}
    </section>
  );
};

const GroupEntries = ({
  groupParam,
  movePending,
  moveError,
}: {
  groupParam: string;
  movePending: boolean;
  moveError: boolean;
}) => {
  const hierarchy = useRequest('getGroupHierarchy', {});

  if (hierarchy.isPending) {
    return (
      <section className="flex min-h-0 w-full flex-1 flex-col border-r md:max-w-md">
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
      title={group.name || 'Untitled group'}
      movePending={movePending}
      moveError={moveError}
    />
  );
};

export const EntryList = ({
  movePending,
  moveError,
}: {
  movePending: boolean;
  moveError: boolean;
}) => {
  const [groupMatch, groupParams] = useRoute<{ group: string }>(getRoute('group'));
  const [tagMatch, tagParams] = useRoute<{ tag: string }>(getRoute('tag'));
  const [trashMatch] = useRoute(getRoute('trash'));

  if (groupMatch) {
    return (
      <GroupEntries
        groupParam={groupParams.group}
        movePending={movePending}
        moveError={moveError}
      />
    );
  }
  if (tagMatch) {
    const tag = decodeRouteParam(tagParams.tag);
    return (
      <EntryQuery
        name="getTagEntries"
        args={{ tag }}
        title={tag}
        movePending={movePending}
        moveError={moveError}
      />
    );
  }
  if (trashMatch) {
    return (
      <EntryQuery
        name="getTrashEntries"
        args={{}}
        title="Trash"
        movePending={movePending}
        moveError={moveError}
      />
    );
  }
  return (
    <EntryQuery
      name="getEntries"
      args={{ excludeTrash: true }}
      title="All Entries"
      movePending={movePending}
      moveError={moveError}
    />
  );
};
