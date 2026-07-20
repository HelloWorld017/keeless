import type { DatabaseNodeId, EntrySummary } from '@keeless/schema';

export const databaseNodeKey = (id: DatabaseNodeId) =>
  `${typeof id === 'number' ? 'int' : 'uuid'}:${id}`;

export const groupDndId = (id: DatabaseNodeId) => `group:${databaseNodeKey(id)}`;

export const entryDndId = (id: DatabaseNodeId) => `entry:${databaseNodeKey(id)}`;

export type EntryDragData = {
  type: 'entry';
  entryId: DatabaseNodeId;
  title: string;
  entry: EntrySummary;
};

export type GroupDragData = {
  type: 'group';
  groupId: DatabaseNodeId;
  title: string;
};

export type TrashDropData = {
  type: 'trash';
  groupId: DatabaseNodeId;
  title: 'Trash';
};
