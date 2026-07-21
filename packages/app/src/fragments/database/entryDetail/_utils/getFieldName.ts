import {EntryFieldKind} from "@keeless/schema";

export const getFieldName = (kind: EntryFieldKind, name: string) => (kind === 'userName' ? 'Username' : name);
