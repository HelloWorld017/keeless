import type { EntryPropertiesUpdate, IconReference } from '@keeless/schema';

export type EntryPropertiesDraft = Omit<EntryPropertiesUpdate, 'icon'> & {
  icon: IconReference;
  tagsChanged: boolean;
};
