import { base64ToBytes, bytesToBase64, bytesToHex, equalBytes, randomBytes } from './binary.ts';
import { CipherId, Compression, KdfId, StandardField } from './constants.ts';
import { KdbxError } from './errors.ts';
import type { VariantDictionary } from './variant-dictionary.ts';

export class KdbxUuid {
  readonly bytes: Uint8Array;

  constructor(value: Uint8Array = randomBytes(16)) {
    if (value.length !== 16) {
      throw new KdbxError('invalid-xml', 'UUID must be 16 bytes');
    }
    this.bytes = value.slice();
  }

  static random(): KdbxUuid {
    return new KdbxUuid();
  }

  static fromBase64(value: string): KdbxUuid {
    return new KdbxUuid(base64ToBytes(value));
  }

  static fromHex(value: string): KdbxUuid {
    if (!/^[\da-f]{32}$/i.test(value)) {
      throw new KdbxError('invalid-xml', 'Invalid UUID');
    }
    return new KdbxUuid(Uint8Array.from(value.match(/.{2}/g)!, part => Number.parseInt(part, 16)));
  }

  equals(other: KdbxUuid): boolean {
    return equalBytes(this.bytes, other.bytes);
  }

  toBase64(): string {
    return bytesToBase64(this.bytes);
  }

  toString(): string {
    return bytesToHex(this.bytes);
  }
}

export interface KdbxValueOptions {
  protected?: boolean;
}

export class KdbxValue {
  value: string;
  protected: boolean;

  constructor(value = '', options: KdbxValueOptions = {}) {
    this.value = value;
    this.protected = options.protected ?? false;
  }
}

export class KdbxTimes {
  creationTime = new Date();
  lastModificationTime = new Date();
  lastAccessTime = new Date();
  expiryTime = new Date(2999, 11, 28, 23, 59, 59);
  expires = false;
  usageCount = 0;
  locationChanged = new Date();
}

export class KdbxBinary {
  data: Uint8Array;
  protected: boolean;

  constructor(data: Uint8Array, options: { protected?: boolean } = {}) {
    this.data = data.slice();
    this.protected = options.protected ?? false;
  }
}

export interface KdbxCustomDataItem {
  value: string;
  protected?: boolean;
  lastModificationTime?: Date;
}

export interface KdbxCustomIcon {
  uuid: KdbxUuid;
  data: Uint8Array;
  name?: string;
  lastModificationTime?: Date;
}

export interface KdbxAutoTypeAssociation {
  window: string;
  keystrokeSequence: string;
}

export class KdbxEntry {
  uuid = KdbxUuid.random();
  iconId = 0;
  customIconUuid?: KdbxUuid;
  foregroundColor = '';
  backgroundColor = '';
  overrideUrl = '';
  tags: string[] = [];
  qualityCheck = true;
  previousParentGroup?: KdbxUuid;
  times = new KdbxTimes();
  fields = new Map<string, KdbxValue>();
  binaries = new Map<string, KdbxBinary>();
  customData = new Map<string, KdbxCustomDataItem>();
  autoTypeEnabled = true;
  autoTypeObfuscation = 0;
  defaultAutoTypeSequence = '';
  autoTypeAssociations: KdbxAutoTypeAssociation[] = [];
  history: KdbxEntry[] = [];

  constructor() {
    this.fields.set(StandardField.Title, new KdbxValue());
    this.fields.set(StandardField.UserName, new KdbxValue());
    this.fields.set(StandardField.Password, new KdbxValue('', { protected: true }));
    this.fields.set(StandardField.URL, new KdbxValue());
    this.fields.set(StandardField.Notes, new KdbxValue());
  }

  get title(): string {
    return this.get(StandardField.Title);
  }

  set title(value: string) {
    this.set(StandardField.Title, value);
  }

  get username(): string {
    return this.get(StandardField.UserName);
  }

  set username(value: string) {
    this.set(StandardField.UserName, value);
  }

  get password(): string {
    return this.get(StandardField.Password);
  }

  set password(value: string) {
    this.set(StandardField.Password, value, { protected: true });
  }

  get url(): string {
    return this.get(StandardField.URL);
  }

  set url(value: string) {
    this.set(StandardField.URL, value);
  }

  get notes(): string {
    return this.get(StandardField.Notes);
  }

  set notes(value: string) {
    this.set(StandardField.Notes, value);
  }

  get(key: string): string {
    return this.fields.get(key)?.value ?? '';
  }

  set(key: string, value: string, options: KdbxValueOptions = {}): this {
    const current = this.fields.get(key);
    this.fields.set(
      key,
      new KdbxValue(value, { protected: options.protected ?? current?.protected ?? false }),
    );
    this.times.lastModificationTime = new Date();
    return this;
  }

  remove(key: string): boolean {
    const removed = this.fields.delete(key);
    if (removed) {
      this.times.lastModificationTime = new Date();
    }
    return removed;
  }
}

export class KdbxGroup {
  uuid = KdbxUuid.random();
  name = '';
  notes = '';
  tags: string[] = [];
  iconId = 48;
  customIconUuid?: KdbxUuid;
  previousParentGroup?: KdbxUuid;
  times = new KdbxTimes();
  isExpanded = true;
  defaultAutoTypeSequence = '';
  enableAutoType: boolean | null = null;
  enableSearching: boolean | null = null;
  lastTopVisibleEntry?: KdbxUuid;
  groups: KdbxGroup[] = [];
  entries: KdbxEntry[] = [];
  customData = new Map<string, KdbxCustomDataItem>();

  constructor(name = '') {
    this.name = name;
  }

  addGroup(name: string): KdbxGroup {
    const group = new KdbxGroup(name);
    this.groups.push(group);
    this.times.lastModificationTime = new Date();
    return group;
  }

  addEntry(entry: KdbxEntry = new KdbxEntry()): KdbxEntry {
    this.entries.push(entry);
    this.times.lastModificationTime = new Date();
    return entry;
  }

  findGroup(uuid: KdbxUuid): KdbxGroup | undefined {
    if (this.uuid.equals(uuid)) {
      return this;
    }
    for (const group of this.groups) {
      const found = group.findGroup(uuid);
      if (found) {
        return found;
      }
    }
    return undefined;
  }

  findEntry(uuid: KdbxUuid): KdbxEntry | undefined {
    const entry = this.entries.find(candidate => candidate.uuid.equals(uuid));
    if (entry) {
      return entry;
    }
    for (const group of this.groups) {
      const found = group.findEntry(uuid);
      if (found) {
        return found;
      }
    }
    return undefined;
  }
}

export class KdbxMeta {
  generator = 'keeless';
  databaseName = '';
  databaseNameChanged = new Date();
  databaseDescription = '';
  databaseDescriptionChanged = new Date();
  defaultUserName = '';
  defaultUserNameChanged = new Date();
  color = '';
  maintenanceHistoryDays = 365;
  masterKeyChanged = new Date();
  masterKeyChangeRec = -1;
  masterKeyChangeForce = -1;
  recycleBinEnabled = true;
  recycleBinUuid = new KdbxUuid(new Uint8Array(16));
  recycleBinChanged = new Date();
  entryTemplatesGroup = new KdbxUuid(new Uint8Array(16));
  entryTemplatesGroupChanged = new Date();
  historyMaxItems = 10;
  historyMaxSize = 6 * 1024 * 1024;
  settingsChanged = new Date();
  lastSelectedGroup = new KdbxUuid(new Uint8Array(16));
  lastTopVisibleGroup = new KdbxUuid(new Uint8Array(16));
  customIcons: KdbxCustomIcon[] = [];
  customData = new Map<string, KdbxCustomDataItem>();
  memoryProtection = {
    title: false,
    userName: false,
    password: true,
    url: false,
    notes: false,
  };
}

export interface KdbxDeletedObject {
  uuid: KdbxUuid;
  deletionTime: Date;
}

export interface KdbxHeaderSettings {
  version: number;
  cipher: (typeof CipherId)[keyof typeof CipherId];
  compression: (typeof Compression)[keyof typeof Compression];
  kdfParameters: VariantDictionary;
  publicCustomData: VariantDictionary;
}

export class KdbxDatabase {
  meta = new KdbxMeta();
  root = new KdbxGroup('Root');
  deletedObjects: KdbxDeletedObject[] = [];
  header: KdbxHeaderSettings = {
    version: 0x00040001,
    cipher: CipherId.Aes256,
    compression: Compression.GZip,
    kdfParameters: new Map<string, Uint8Array | number | bigint>([
      ['$UUID', Uint8Array.from(KdfId.Argon2id.match(/.{2}/g)!, part => Number.parseInt(part, 16))],
      ['V', 0x13],
      ['S', randomBytes(32)],
      ['I', 3n],
      ['M', 64n * 1024n * 1024n],
      ['P', 1],
    ]),
    publicCustomData: new Map(),
  };

  static create(name = ''): KdbxDatabase {
    const database = new KdbxDatabase();
    database.meta.databaseName = name;
    return database;
  }

  findGroup(uuid: KdbxUuid): KdbxGroup | undefined {
    return this.root.findGroup(uuid);
  }

  findEntry(uuid: KdbxUuid): KdbxEntry | undefined {
    return this.root.findEntry(uuid);
  }

  removeEntry(uuid: KdbxUuid): boolean {
    const removed = removeEntryFromGroup(this.root, uuid);
    if (removed) {
      this.deletedObjects.push({ uuid, deletionTime: new Date() });
    }
    return removed;
  }

  removeGroup(uuid: KdbxUuid): boolean {
    const removed = removeGroupFromGroup(this.root, uuid);
    if (removed) {
      this.deletedObjects.push({ uuid, deletionTime: new Date() });
    }
    return removed;
  }
}

function removeEntryFromGroup(group: KdbxGroup, uuid: KdbxUuid): boolean {
  const index = group.entries.findIndex(entry => entry.uuid.equals(uuid));
  if (index >= 0) {
    group.entries.splice(index, 1);
    return true;
  }
  return group.groups.some(child => removeEntryFromGroup(child, uuid));
}

function removeGroupFromGroup(group: KdbxGroup, uuid: KdbxUuid): boolean {
  const index = group.groups.findIndex(child => child.uuid.equals(uuid));
  if (index >= 0) {
    group.groups.splice(index, 1);
    return true;
  }
  return group.groups.some(child => removeGroupFromGroup(child, uuid));
}
