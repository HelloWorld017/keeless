import type { EntryPropertiesUpdate } from '@keeless/schema';

export type EntryPropertiesDraft = Omit<EntryPropertiesUpdate, 'tags'> & {
  tags: string;
  tagsChanged: boolean;
};
