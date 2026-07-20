import {STANDARD_NAMES} from "../_constants";

const STANDARD_NAMES_MAP: Record<string, string> = {
  'Title': 'Title',
  'Password': 'Password',
  'URL': 'URL',
  'UserName': 'Username',
  'Notes': 'Notes',
} satisfies Record<typeof STANDARD_NAMES[number], string>;

export const getFieldName = (name: string) =>
  Object.hasOwn(STANDARD_NAMES_MAP, name) ? STANDARD_NAMES_MAP[name] : name;
