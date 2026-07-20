import type { DatabaseNodeId, EntrySummary, IconReference } from '@keeless/schema';

export const databaseNodeKey = (id: DatabaseNodeId) =>
  `${typeof id === 'number' ? 'int' : 'uuid'}:${id}`;

export const groupDndId = (id: DatabaseNodeId) => `group:${databaseNodeKey(id)}`;

export const entryDndId = (id: DatabaseNodeId) => `entry:${databaseNodeKey(id)}`;

export type EntryDragSource =
  | { type: 'all' }
  | { type: 'group'; groupId: DatabaseNodeId }
  | { type: 'tag' }
  | { type: 'trash' };

export type EntryDragData = {
  type: 'entry';
  entryId: DatabaseNodeId;
  title: string;
  entry: EntrySummary;
  source: EntryDragSource;
};

export type GroupDragData = {
  type: 'group';
  groupId: DatabaseNodeId;
  title: string;
  icon: IconReference;
};

export type TrashDropData = {
  type: 'trash';
  groupId: DatabaseNodeId;
  title: 'Trash';
};

export type DragDropData = EntryDragData | GroupDragData | TrashDropData;
