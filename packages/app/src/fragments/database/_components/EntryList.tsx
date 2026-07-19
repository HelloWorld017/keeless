import { Alert, AlertDescription, AlertTitle } from '@/components/alert';
import {
  Item,
  ItemContent,
  ItemDescription,
  ItemGroup,
  ItemMedia,
  ItemTitle,
} from '@/components/item';
import { Skeleton } from '@/components/skeleton';
import { useRequest } from '@/fragments/_providers/QueryProvider';
import { IconAlertCircle, IconFile } from '@/icons';
import { getRoute } from '@/utils/route';
import { useDraggable } from '@dnd-kit/core';
import { useRoute, useSearchParams } from 'wouter';
import { entryDndId, type EntryDragData } from './dnd';
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

const EntryRow = ({
  entry,
  selected,
  disabled,
  onSelect,
}: {
  entry: EntrySummary;
  selected: boolean;
  disabled: boolean;
  onSelect: () => void;
}) => {
  const title = entry.name || (entry.nameIsProtected ? 'Protected entry' : 'Untitled entry');
  const description = entry.url || (entry.urlIsProtected ? 'Protected URL' : undefined);
  const data: EntryDragData = { type: 'entry', entryId: entry.id, title, description };
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: entryDndId(entry.id),
    data,
    disabled,
  });

  return (
    <Item
      ref={setNodeRef}
      render={<button type="button" aria-label={title} />}
      variant={selected ? 'muted' : 'default'}
      className={isDragging ? 'z-10 cursor-grabbing opacity-60' : 'cursor-grab'}
      onClick={onSelect}
      {...attributes}
      {...listeners}
      aria-pressed={selected}
    >
      <ItemMedia variant="icon">
        <IconFile />
      </ItemMedia>
      <ItemContent className="min-w-0 gap-0.5">
        <ItemTitle>{title}</ItemTitle>
        {description && <ItemDescription className="line-clamp-1">{description}</ItemDescription>}
      </ItemContent>
    </Item>
  );
};

const EntryListSkeleton = () => (
  <div className="space-y-2 p-3" aria-label="Loading entries">
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

      {entries.isPending && <EntryListSkeleton />}
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
        <div className="min-h-0 overflow-y-auto p-3">
          <ItemGroup className="gap-1">
            {result.entries.map(entry => (
              <EntryRow
                key={entryDndId(entry.id)}
                entry={entry}
                selected={selectedEntry === String(entry.id)}
                disabled={movePending}
                onSelect={() => setSearchParams({ entry: String(entry.id) }, { replace: true })}
              />
            ))}
          </ItemGroup>
        </div>
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
        <EntryListSkeleton />
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
