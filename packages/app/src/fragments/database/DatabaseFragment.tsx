import { SidebarInset, SidebarProvider, SidebarTrigger } from '@/components/sidebar';
import { useRequestClient } from '@/fragments/_providers/QueryProvider';
import {
  DndContext,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
  type KeyboardCoordinateGetter,
} from '@dnd-kit/core';
import { sortableKeyboardCoordinates } from '@dnd-kit/sortable';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { EntryItem } from './_components/EntryItem';
import { EntryList } from './_components/EntryList';
import { Sidebar } from './_components/Sidebar';
import type { EntryDragData } from './_utils/dragAndDrop';
import type { MoveEntryArgs } from '@keeless/schema';

const keyboardDirections = ['ArrowDown', 'ArrowRight', 'ArrowUp', 'ArrowLeft'] as const;

const isKeyboardDirection = (code: string): code is (typeof keyboardDirections)[number] =>
  keyboardDirections.includes(code as (typeof keyboardDirections)[number]);

const keyboardCoordinates: KeyboardCoordinateGetter = (event, args) => {
  if (args.context.active?.data.current?.type !== 'entry') {
    return sortableKeyboardCoordinates(event, args);
  }
  if (!isKeyboardDirection(event.code)) {
    return undefined;
  }

  event.preventDefault();
  const { collisionRect, droppableContainers, droppableRects, over } = args.context;
  if (!collisionRect) {
    return undefined;
  }
  const referenceRect = (over && droppableRects.get(over.id)) || collisionRect;
  const referenceCenter = {
    x: referenceRect.left + referenceRect.width / 2,
    y: referenceRect.top + referenceRect.height / 2,
  };
  const candidates = droppableContainers
    .getEnabled()
    .filter(container => {
      const data = container.data.current;
      const rect = droppableRects.get(container.id);
      if (!rect || (data?.type !== 'group' && data?.type !== 'trash')) {
        return false;
      }
      const center = { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
      switch (event.code) {
        case 'ArrowDown':
          return center.y > referenceCenter.y;
        case 'ArrowRight':
          return center.x > referenceCenter.x;
        case 'ArrowUp':
          return center.y < referenceCenter.y;
        case 'ArrowLeft':
          return center.x < referenceCenter.x;
        default:
          return false;
      }
    })
    .map(container => droppableRects.get(container.id)!);
  const distance = (rect: (typeof candidates)[number]) =>
    Math.hypot(
      rect.left + rect.width / 2 - referenceCenter.x,
      rect.top + rect.height / 2 - referenceCenter.y,
    );
  const target = candidates.reduce<(typeof candidates)[number] | undefined>(
    (closest, candidate) =>
      !closest || distance(candidate) < distance(closest) ? candidate : closest,
    undefined,
  );
  if (!target) {
    return undefined;
  }
  return {
    x: target.left + (target.width - collisionRect.width) / 2,
    y: target.top + (target.height - collisionRect.height) / 2,
  };
};

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

export const DatabaseFragment = () => {
  const requestClient = useRequestClient();
  const queryClient = useQueryClient();
  const [activeEntry, setActiveEntry] = useState<EntryDragData>();
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: keyboardCoordinates }),
  );
  const moveEntry = useMutation({
    mutationFn: (args: MoveEntryArgs) => requestClient.data!.request('moveEntry', args),
    onSettled: () =>
      Promise.all(
        entryQueryNames.map(name => queryClient.invalidateQueries({ queryKey: ['request', name] })),
      ),
  });
  const handleDragStart = ({ active }: DragStartEvent) => {
    const data = active.data.current;
    if (data?.type === 'entry') {
      setActiveEntry({
        type: 'entry',
        entryId: data.entryId,
        title: data.title,
        entry: data.entry,
      });
    }
  };
  const handleDragEnd = ({ active, over }: DragEndEvent) => {
    setActiveEntry(undefined);
    const activeData = active.data.current;
    const overData = over?.data.current;
    if (
      activeData?.type !== 'entry' ||
      (overData?.type !== 'group' && overData?.type !== 'trash')
    ) {
      return;
    }
    moveEntry.mutate({ entryId: activeData.entryId, parentGroupId: overData.groupId });
  };

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      accessibility={{ announcements }}
      onDragStart={handleDragStart}
      onDragCancel={() => setActiveEntry(undefined)}
      onDragEnd={handleDragEnd}
    >
      <SidebarProvider>
        <Sidebar />
        <SidebarInset className="h-svh overflow-hidden">
          <div className="flex min-h-0 flex-1">
            <EntryList movePending={moveEntry.isPending} moveError={moveEntry.isError} />
          </div>
        </SidebarInset>
      </SidebarProvider>
      <DragOverlay>
        {activeEntry && (
          <EntryItem
            entry={activeEntry.entry}
            variant="outline"
            className="w-80 bg-background shadow-lg"
          />
        )}
      </DragOverlay>
    </DndContext>
  );
};
