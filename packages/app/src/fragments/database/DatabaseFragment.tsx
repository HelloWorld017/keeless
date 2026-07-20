import { SidebarInset, SidebarProvider } from '@/components/sidebar';
import { useRequest, useRequestClient } from '@/fragments/_providers/QueryProvider';
import { cx } from '@/utils/css';
import { buildRoute } from '@/utils/route';
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  closestCenter,
  pointerWithin,
  useSensor,
  useSensors,
  type CollisionDetection,
  type DropAnimation,
  type DragEndEvent,
  type DragOverEvent,
  type DragStartEvent,
  type Modifier,
} from '@dnd-kit/core';
import { snapCenterToCursor } from '@dnd-kit/modifiers';
import { CSS } from '@dnd-kit/utilities';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useRef, useState } from 'react';
import { Redirect, useSearchParams } from 'wouter';
import { EntryItem } from './_components/EntryItem';
import { EntryList } from './_components/EntryList';
import { GroupDragOverlay } from './_components/GroupTree';
import { Sidebar } from './_components/Sidebar';
import { databaseNodeKey, type DragDropData, type EntryDragData } from './_utils/dragAndDrop';
import type { MoveEntryArgs } from '@keeless/schema';

const collisionDetection: CollisionDetection = args =>
  args.active.data.current?.type === 'entry' && args.pointerCoordinates
    ? pointerWithin(args)
    : closestCenter(args);

const snapEntryCenterToCursor: Modifier = args =>
  args.active?.data.current?.type === 'entry' ? snapCenterToCursor(args) : args.transform;

const entryOverlayModifiers = [snapEntryCenterToCursor];

const announcements = {
  onDragStart: ({ active }: DragStartEvent) => {
    const data = active.data.current;
    return data?.title ? `Picked up ${data.title}.` : undefined;
  },

  onDragOver: ({ over }: { over: DragEndEvent['over'] }) => {
    const title = over?.data.current?.title;
    return title ? `Over ${title}.` : 'Not over a destination.';
  },

  onDragEnd: ({ active, over }: DragEndEvent) => {
    const activeTitle = active.data.current?.title;
    const overTitle = over?.data.current?.title;
    return activeTitle && overTitle ? `Dropped ${activeTitle} in ${overTitle}.` : 'Drag cancelled.';
  },

  onDragCancel: ({ active }: DragEndEvent) => {
    const title = active.data.current?.title;
    return title ? `Cancelled dragging ${title}.` : 'Drag cancelled.';
  },
};

const entryQueryNames = [
  'getEntries',
  'getGroupEntries',
  'getTagEntries',
  'getTrashEntries',
  'getTags',
  'getEntryDetail',
] as const;

const DatabaseFragmentContents = () => {
  const requestClient = useRequestClient();
  const queryClient = useQueryClient();
  const [activeDrag, setActiveDrag] = useState<DragDropData | null>(null);
  const [entryOverGroup, setEntryOverGroup] = useState(false);
  const [movingEntry, setMovingEntry] = useState<Pick<EntryDragData, 'entryId' | 'source'>>();
  const validEntryDrop = useRef(false);
  const activeEntry: EntryDragData | null = activeDrag?.type === 'entry' ? activeDrag : null;
  const dropAnimation: DropAnimation = {
    keyframes: ({ active, transform }) => {
      const initial = { transform: CSS.Transform.toString(transform.initial) };

      if (active.data.current?.type === 'group') {
        return [initial, initial];
      }

      return validEntryDrop.current
        ? [initial, { ...initial, opacity: 0 }]
        : [initial, { transform: CSS.Transform.toString(transform.final), opacity: 0 }];
    },
  };
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }));
  const moveEntry = useMutation({
    mutationFn: (args: MoveEntryArgs) => requestClient.data!.request('moveEntry', args),
    onSettled: async () => {
      try {
        await Promise.all(
          entryQueryNames.map(name =>
            queryClient.invalidateQueries({ queryKey: ['request', name] }),
          ),
        );
      } finally {
        setMovingEntry(undefined);
      }
    },
  });

  const clearDrag = () => {
    setEntryOverGroup(false);
    setActiveDrag(null);
  };

  const handleDragStart = ({ active }: DragStartEvent) => {
    validEntryDrop.current = false;
    const data = active.data.current as DragDropData | undefined;
    if (data?.type === 'entry') {
      setActiveDrag({
        type: 'entry',
        entryId: data.entryId,
        title: data.title,
        entry: data.entry,
        source: data.source,
      });
      return;
    }
    setActiveDrag(data ?? null);
  };
  const handleDragOver = ({ active, over }: DragOverEvent) => {
    const activeData = active.data.current as DragDropData | undefined;
    const overData = over?.data.current as DragDropData | undefined;
    if (activeData?.type !== 'entry') {
      return;
    }

    const isOverGroup = overData?.type === 'group' || overData?.type === 'trash';
    setEntryOverGroup(current => (current === isOverGroup ? current : isOverGroup));
  };

  const handleDragEnd = ({ active, over }: DragEndEvent) => {
    const activeData = active.data.current as DragDropData | undefined;
    const overData = over?.data.current as DragDropData | undefined;

    if (
      activeData?.type !== 'entry' ||
      (overData?.type !== 'group' && overData?.type !== 'trash')
    ) {
      validEntryDrop.current = false;
      clearDrag();
      return;
    }

    const sameDestination =
      (activeData.source.type === 'group' &&
        overData.type === 'group' &&
        databaseNodeKey(activeData.source.groupId) === databaseNodeKey(overData.groupId)) ||
      (activeData.source.type === 'trash' && overData.type === 'trash');

    if (sameDestination) {
      validEntryDrop.current = false;
      clearDrag();
      return;
    }

    validEntryDrop.current = true;
    const leavesCurrentList =
      activeData.source.type === 'group' ||
      (activeData.source.type === 'trash' && overData.type === 'group') ||
      ((activeData.source.type === 'all' || activeData.source.type === 'tag') &&
        overData.type === 'trash');

    if (leavesCurrentList) {
      setMovingEntry({ entryId: activeData.entryId, source: activeData.source });
    }

    clearDrag();
    moveEntry.mutate({ entryId: activeData.entryId, parentGroupId: overData.groupId });
  };

  const [searchParams] = useSearchParams();
  const selectedEntry = searchParams.get('entry');

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={collisionDetection}
      accessibility={{ announcements }}
      onDragStart={handleDragStart}
      onDragOver={handleDragOver}
      onDragCancel={() => {
        validEntryDrop.current = false;
        clearDrag();
      }}
      onDragEnd={handleDragEnd}
    >
      <SidebarProvider className="[--sidebar-width:16rem]! xl:[--sidebar-width:18rem]!">
        <Sidebar />
        <SidebarInset className="h-svh overflow-hidden">
          <div className="flex min-h-0 flex-1">
            <EntryList
              movePending={moveEntry.isPending}
              moveError={moveEntry.isError}
              hiddenEntry={movingEntry}
            />
          </div>
        </SidebarInset>
      </SidebarProvider>
      <DragOverlay
        modifiers={activeEntry ? entryOverlayModifiers : undefined}
        dropAnimation={dropAnimation}
        style={activeEntry ? { width: 320, height: 64 } : undefined}
      >
        {activeEntry ? (
          <EntryItem
            entry={activeEntry.entry}
            selected={selectedEntry === String(activeEntry.entry.id)}
            className={cx(
              'w-full opacity-75 transition-opacity transition-transform',
              entryOverGroup && 'scale-75',
            )}
          />
        ) : activeDrag?.type === 'group' ? (
          <GroupDragOverlay title={activeDrag.title} icon={activeDrag.icon} />
        ) : null}
      </DragOverlay>
    </DndContext>
  );
};

export const DatabaseFragment = () => {
  const databaseStatus = useRequest('getDatabaseStatus', {});

  if (databaseStatus.data?.status === 'unlocked') {
    return <DatabaseFragmentContents />;
  }

  if (databaseStatus.isPending || databaseStatus.isFetching) {
    return null;
  }

  return <Redirect to={buildRoute('open')} replace />;
};
