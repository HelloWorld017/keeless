import { Button } from '@/components/button';
import {
  SidebarEmpty,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from '@/components/sidebar';
import { IconFolder, IconGripVertical } from '@/icons';
import { cx } from '@/utils/css';
import { buildRoute } from '@/utils/route';
import {
  useDndMonitor,
  type DragEndEvent,
  type DragMoveEvent,
  type DragOverEvent,
  type DragStartEvent,
} from '@dnd-kit/core';
import {
  SortableContext,
  arrayMove,
  defaultAnimateLayoutChanges,
  useSortable,
  verticalListSortingStrategy,
  type AnimateLayoutChanges,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { useState } from 'react';
import { Link } from 'wouter';
import { groupDndId, type GroupDragData } from '../_utils/dragAndDrop';
import type {
  DatabaseNodeId,
  GroupHierarchyItem,
  GroupHierarchyResult,
  MoveGroupArgs,
} from '@keeless/schema';

const INDENTATION_WIDTH = 16;

const animateLayoutChanges: AnimateLayoutChanges = args =>
  args.active?.data.current?.type === 'group' ? args.isSorting : defaultAnimateLayoutChanges(args);

const nodeKey = groupDndId;

type FlatGroup = {
  id: DatabaseNodeId;
  key: string;
  parentId: DatabaseNodeId;
  parentKey: string;
  depth: number;
  ancestors: string[];
  group: GroupHierarchyItem;
};

type DropIndicator = {
  depth: number;
  edge: 'before' | 'after';
};

const flattenHierarchy = (hierarchy: GroupHierarchyResult) => {
  const groups = new Map(hierarchy.groups.map(group => [nodeKey(group.id), group]));
  const hidden = new Set<string>();
  const hide = (id: DatabaseNodeId) => {
    const key = nodeKey(id);
    if (hidden.has(key)) {
      return;
    }
    hidden.add(key);
    groups.get(key)?.childGroupIds.forEach(hide);
  };
  if (hierarchy.recycleBinId !== null) {
    hide(hierarchy.recycleBinId);
  }

  const flattened: FlatGroup[] = [];
  const visited = new Set<string>();
  const visit = (
    id: DatabaseNodeId,
    parentId: DatabaseNodeId,
    depth: number,
    ancestors: string[],
  ) => {
    const key = nodeKey(id);
    if (hidden.has(key) || visited.has(key)) {
      return;
    }
    const group = groups.get(key);
    if (!group) {
      return;
    }
    visited.add(key);
    flattened.push({
      id,
      key,
      parentId,
      parentKey: nodeKey(parentId),
      depth,
      ancestors,
      group,
    });
    group.childGroupIds.forEach(childId => visit(childId, id, depth + 1, [...ancestors, key]));
  };

  const root = groups.get(nodeKey(hierarchy.rootGroupId));
  root?.childGroupIds.forEach(id => visit(id, hierarchy.rootGroupId, 0, []));
  return flattened;
};

const getProjection = (
  items: FlatGroup[],
  activeKey: string,
  overKey: string,
  dragOffset: number,
  rootGroupId: DatabaseNodeId,
) => {
  const activeIndex = items.findIndex(item => item.key === activeKey);
  const overIndex = items.findIndex(item => item.key === overKey);
  const activeItem = items[activeIndex];
  if (!activeItem || overIndex < 0) {
    return undefined;
  }

  const reordered = arrayMove(items, activeIndex, overIndex);
  const previous = reordered[overIndex - 1];
  const next = reordered[overIndex + 1];
  const maxDepth = previous ? previous.depth + 1 : 0;
  const minDepth = next?.depth ?? 0;
  const requestedDepth = activeItem.depth + Math.round(dragOffset / INDENTATION_WIDTH);
  const depth = Math.max(minDepth, Math.min(requestedDepth, maxDepth));

  let parentId = rootGroupId;
  if (depth > 0 && previous) {
    if (depth > previous.depth) {
      parentId = previous.id;
    } else if (depth === previous.depth) {
      parentId = previous.parentId;
    } else {
      parentId =
        reordered
          .slice(0, overIndex)
          .reduceRight<DatabaseNodeId | undefined>(
            (match, item) => match ?? (item.depth === depth ? item.parentId : undefined),
            undefined,
          ) ?? rootGroupId;
    }
  }

  return { depth, parentId, reordered, index: overIndex };
};

export const moveGroupInHierarchy = (
  hierarchy: GroupHierarchyResult,
  { groupId, parentGroupId, destinationIndex }: MoveGroupArgs,
) => {
  const movedKey = nodeKey(groupId);
  const parentKey = nodeKey(parentGroupId);
  return {
    ...hierarchy,
    groups: hierarchy.groups.map(group => {
      const children = group.childGroupIds.filter(id => nodeKey(id) !== movedKey);
      if (nodeKey(group.id) === parentKey) {
        children.splice(destinationIndex, 0, groupId);
      }
      return { ...group, childGroupIds: children };
    }),
  };
};

export const GroupDragOverlay = ({ title }: { title: string }) => (
  <div className="flex h-8 w-full items-center gap-2 overflow-hidden rounded-md border border-sidebar-border bg-sidebar px-2 text-sm text-sidebar-foreground shadow-lg opacity-50">
    <IconFolder className="size-4 shrink-0" />
    <span className="truncate">{title}</span>
  </div>
);

const SortableGroup = ({
  item,
  depth,
  active,
  disabled,
  indicator,
  onNavigate,
}: {
  item: FlatGroup;
  depth: number;
  active: boolean;
  disabled: boolean;
  indicator?: DropIndicator;
  onNavigate: () => void;
}) => {
  const data: GroupDragData = {
    type: 'group',
    groupId: item.id,
    title: item.group.name || 'Untitled group',
  };
  const {
    active: draggedItem,
    attributes,
    listeners,
    setActivatorNodeRef,
    setNodeRef,
    transform,
    transition,
    isDragging,
    isOver,
  } = useSortable({ id: item.key, data, disabled, animateLayoutChanges });
  const isEntryOver = isOver && draggedItem?.data.current?.type === 'entry';
  return (
    <SidebarMenuItem
      ref={setNodeRef}
      className={isDragging ? 'z-10' : undefined}
      style={{
        paddingLeft: depth * INDENTATION_WIDTH,
        transform: CSS.Transform.toString(transform),
        transition,
      }}
    >
      {indicator && (
        <div
          className={cx(
            'pointer-events-none absolute right-1 z-20 h-0.5 bg-primary',
            indicator.edge === 'before' ? '-top-px' : '-bottom-px',
          )}
          style={{ left: indicator.depth * INDENTATION_WIDTH + 8 }}
        ></div>
      )}
      <SidebarMenuButton
        render={
          <Link href={buildRoute('group', { group: String(item.id) })} replace draggable={false} />
        }
        isActive={active}
        className={cx(
          'border-2 border-transparent pr-8',
          isEntryOver && 'border-sidebar-ring',
          isDragging && 'opacity-0',
        )}
        onClick={onNavigate}
      >
        <IconFolder />
        <span>{item.group.name || 'Untitled group'}</span>
      </SidebarMenuButton>
      <Button
        ref={setActivatorNodeRef}
        type="button"
        variant="ghost"
        size="icon-xs"
        className={cx(
          'absolute top-1 right-1 cursor-grab touch-none text-sidebar-foreground/60 hover:bg-sidebar-accent active:translate-y-0',
          isDragging && 'cursor-grabbing opacity-0',
        )}
        aria-label={`Move ${item.group.name || 'untitled group'}`}
        {...attributes}
        {...listeners}
      >
        <IconGripVertical />
      </Button>
    </SidebarMenuItem>
  );
};

export const GroupTree = ({
  hierarchy,
  location,
  disabled,
  onMove,
  onNavigate,
}: {
  hierarchy: GroupHierarchyResult;
  location: string;
  disabled: boolean;
  onMove: (args: MoveGroupArgs) => void;
  onNavigate: () => void;
}) => {
  const flattened = flattenHierarchy(hierarchy);
  const [activeKey, setActiveKey] = useState<string>();
  const [overKey, setOverKey] = useState<string>();
  const [dragOffset, setDragOffset] = useState(0);
  const items = activeKey
    ? flattened.filter(item => !item.ancestors.includes(activeKey))
    : flattened;
  const projection =
    activeKey && overKey
      ? getProjection(items, activeKey, overKey, dragOffset, hierarchy.rootGroupId)
      : undefined;
  const previousItem = projection?.reordered[projection.index - 1];
  const indicator: (DropIndicator & { key: string }) | undefined =
    projection && overKey
      ? {
          key: previousItem?.key ?? overKey,
          depth: projection.depth,
          edge: previousItem ? 'after' : 'before',
        }
      : undefined;

  const reset = () => {
    setActiveKey(undefined);
    setOverKey(undefined);
    setDragOffset(0);
  };
  const handleDragStart = ({ active }: DragStartEvent) => {
    if (active.data.current?.type !== 'group') {
      return;
    }
    setActiveKey(String(active.id));
    setOverKey(String(active.id));
  };
  const handleDragMove = ({ active, delta }: DragMoveEvent) => {
    if (active.data.current?.type === 'group') {
      setDragOffset(delta.x);
    }
  };
  const handleDragOver = ({ active, over }: DragOverEvent) => {
    if (active.data.current?.type === 'group') {
      setOverKey(over ? String(over.id) : undefined);
    }
  };
  const handleDragEnd = ({ active, over }: DragEndEvent) => {
    if (active.data.current?.type !== 'group') {
      return;
    }
    if (!over || !projection) {
      reset();
      return;
    }
    const activeItem = items.find(item => item.key === String(active.id));
    if (!activeItem) {
      reset();
      return;
    }
    const parentKey = nodeKey(projection.parentId);
    const destinationIndex = projection.reordered
      .slice(0, projection.index)
      .filter(item => item.key !== activeItem.key && item.parentKey === parentKey).length;
    const currentIndex = items
      .slice(
        0,
        items.findIndex(item => item.key === activeItem.key),
      )
      .filter(item => item.parentKey === activeItem.parentKey).length;
    if (parentKey === activeItem.parentKey && destinationIndex === currentIndex) {
      reset();
      return;
    }
    onMove({
      groupId: activeItem.id,
      parentGroupId: projection.parentId,
      destinationIndex,
    });
    reset();
  };
  useDndMonitor({
    onDragStart: handleDragStart,
    onDragMove: handleDragMove,
    onDragOver: handleDragOver,
    onDragCancel: event => {
      if (event.active.data.current?.type === 'group') {
        reset();
      }
    },
    onDragEnd: handleDragEnd,
  });

  if (flattened.length === 0) {
    return <SidebarEmpty>No groups</SidebarEmpty>;
  }

  return (
    <SortableContext items={items.map(item => item.key)} strategy={verticalListSortingStrategy}>
      <SidebarMenu>
        {items.map(item => (
          <SortableGroup
            key={item.key}
            item={item}
            depth={item.key === activeKey && projection ? projection.depth : item.depth}
            active={location === buildRoute('group', { group: String(item.id) })}
            disabled={disabled}
            indicator={indicator?.key === item.key ? indicator : undefined}
            onNavigate={onNavigate}
          />
        ))}
      </SidebarMenu>
    </SortableContext>
  );
};
