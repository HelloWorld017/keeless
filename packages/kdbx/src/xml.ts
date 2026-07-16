import {
  DOMParser,
  type Document as XmlDocument,
  type Element as XmlElement,
  type Node as XmlNode,
  XMLSerializer,
} from '@xmldom/xmldom';
import {
  base64ToBytes,
  BinaryReader,
  BinaryWriter,
  bytesToBase64,
  decodeUtf8,
  utf8,
} from './binary.ts';
import { KdbxError } from './errors.ts';
import { KdbxDatabase, KdbxEntry, KdbxGroup, KdbxTimes, KdbxUuid, KdbxValue } from './model.ts';
import type { KdbxBinary, KdbxCustomDataItem } from './model.ts';

export interface ProtectedStream {
  process(value: Uint8Array): Uint8Array;
}

const secondsFromYearOneToUnixEpoch = 62_135_596_800n;

export function parseDatabaseXml(
  xml: string,
  protectedStream: ProtectedStream | undefined,
  binaries: KdbxBinary[],
): KdbxDatabase {
  const document = new DOMParser({ onError: () => undefined }).parseFromString(
    xml,
    'application/xml',
  );
  const root = document.documentElement;
  if (!root || root.tagName !== 'KeePassFile' || root.getElementsByTagName('parsererror').length) {
    throw new KdbxError('invalid-xml', 'Invalid KeePass XML document');
  }
  const database = new KdbxDatabase();
  const meta = child(root, 'Meta');
  const rootElement = child(root, 'Root');
  if (!meta || !rootElement) {
    throw new KdbxError('invalid-xml', 'KeePass XML is missing Meta or Root');
  }
  parseMeta(meta, database, protectedStream);
  const group = child(rootElement, 'Group');
  if (!group) {
    throw new KdbxError('invalid-xml', 'KeePass XML is missing the root group');
  }
  database.root = parseGroup(group, protectedStream, binaries);
  const deletedObjects = child(rootElement, 'DeletedObjects');
  if (deletedObjects) {
    for (const element of children(deletedObjects, 'DeletedObject')) {
      const uuid = textChild(element, 'UUID');
      const deletionTime = textChild(element, 'DeletionTime');
      if (uuid && deletionTime) {
        database.deletedObjects.push({
          uuid: KdbxUuid.fromBase64(uuid),
          deletionTime: parseDate(deletionTime),
        });
      }
    }
  }
  return database;
}

export function serializeDatabaseXml(
  database: KdbxDatabase,
  protectedStream: ProtectedStream,
  binaries: KdbxBinary[],
): string {
  const document = new DOMParser().parseFromString('<KeePassFile/>', 'application/xml');
  const root = document.documentElement;
  if (!root) {
    throw new KdbxError('invalid-xml', 'Unable to create KeePass XML document');
  }
  root.appendChild(serializeMeta(document, database, protectedStream));
  const rootElement = appendElement(document, root, 'Root');
  rootElement.appendChild(serializeGroup(document, database.root, protectedStream, binaries));
  const deletedObjects = appendElement(document, rootElement, 'DeletedObjects');
  for (const deleted of database.deletedObjects) {
    const element = appendElement(document, deletedObjects, 'DeletedObject');
    appendText(document, element, 'UUID', deleted.uuid.toBase64());
    appendText(document, element, 'DeletionTime', serializeDate(deleted.deletionTime));
  }
  return `<?xml version="1.0" encoding="utf-8" standalone="yes"?>${new XMLSerializer().serializeToString(root)}`;
}

function parseMeta(
  element: XmlElement,
  database: KdbxDatabase,
  protectedStream: ProtectedStream | undefined,
): void {
  const meta = database.meta;
  meta.generator = textChild(element, 'Generator') || meta.generator;
  meta.databaseName = textChild(element, 'DatabaseName');
  meta.databaseNameChanged = dateChild(element, 'DatabaseNameChanged') ?? meta.databaseNameChanged;
  meta.databaseDescription = textChild(element, 'DatabaseDescription');
  meta.databaseDescriptionChanged =
    dateChild(element, 'DatabaseDescriptionChanged') ?? meta.databaseDescriptionChanged;
  meta.defaultUserName = textChild(element, 'DefaultUserName');
  meta.defaultUserNameChanged =
    dateChild(element, 'DefaultUserNameChanged') ?? meta.defaultUserNameChanged;
  meta.color = textChild(element, 'Color');
  meta.maintenanceHistoryDays = numberChild(element, 'MaintenanceHistoryDays', 365);
  meta.masterKeyChanged = dateChild(element, 'MasterKeyChanged') ?? meta.masterKeyChanged;
  meta.masterKeyChangeRec = numberChild(element, 'MasterKeyChangeRec', -1);
  meta.masterKeyChangeForce = numberChild(element, 'MasterKeyChangeForce', -1);
  meta.recycleBinEnabled = booleanChild(element, 'RecycleBinEnabled', true);
  meta.recycleBinUuid = uuidChild(element, 'RecycleBinUUID') ?? meta.recycleBinUuid;
  meta.recycleBinChanged = dateChild(element, 'RecycleBinChanged') ?? meta.recycleBinChanged;
  meta.entryTemplatesGroup = uuidChild(element, 'EntryTemplatesGroup') ?? meta.entryTemplatesGroup;
  meta.entryTemplatesGroupChanged =
    dateChild(element, 'EntryTemplatesGroupChanged') ?? meta.entryTemplatesGroupChanged;
  meta.historyMaxItems = numberChild(element, 'HistoryMaxItems', 10);
  meta.historyMaxSize = numberChild(element, 'HistoryMaxSize', 6 * 1024 * 1024);
  meta.lastSelectedGroup = uuidChild(element, 'LastSelectedGroup') ?? meta.lastSelectedGroup;
  meta.lastTopVisibleGroup = uuidChild(element, 'LastTopVisibleGroup') ?? meta.lastTopVisibleGroup;
  meta.settingsChanged = dateChild(element, 'SettingsChanged') ?? meta.settingsChanged;
  const memory = child(element, 'MemoryProtection');
  if (memory) {
    meta.memoryProtection.title = booleanChild(memory, 'ProtectTitle', false);
    meta.memoryProtection.userName = booleanChild(memory, 'ProtectUserName', false);
    meta.memoryProtection.password = booleanChild(memory, 'ProtectPassword', true);
    meta.memoryProtection.url = booleanChild(memory, 'ProtectURL', false);
    meta.memoryProtection.notes = booleanChild(memory, 'ProtectNotes', false);
  }
  const customIcons = child(element, 'CustomIcons');
  if (customIcons) {
    for (const icon of children(customIcons, 'Icon')) {
      const uuid = uuidChild(icon, 'UUID');
      const data = textChild(icon, 'Data');
      if (uuid && data) {
        meta.customIcons.push({
          uuid,
          data: base64ToBytes(data),
          name: textChild(icon, 'Name') || undefined,
          lastModificationTime: dateChild(icon, 'LastModificationTime'),
        });
      }
    }
  }
  meta.customData = parseCustomData(child(element, 'CustomData'), protectedStream);
}

function serializeMeta(
  document: XmlDocument,
  database: KdbxDatabase,
  protectedStream: ProtectedStream,
): XmlElement {
  const element = document.createElement('Meta');
  const meta = database.meta;
  appendText(document, element, 'Generator', meta.generator);
  appendText(document, element, 'SettingsChanged', serializeDate(meta.settingsChanged));
  appendText(document, element, 'DatabaseName', meta.databaseName);
  appendText(document, element, 'DatabaseNameChanged', serializeDate(meta.databaseNameChanged));
  appendText(document, element, 'DatabaseDescription', meta.databaseDescription);
  appendText(
    document,
    element,
    'DatabaseDescriptionChanged',
    serializeDate(meta.databaseDescriptionChanged),
  );
  appendText(document, element, 'DefaultUserName', meta.defaultUserName);
  appendText(
    document,
    element,
    'DefaultUserNameChanged',
    serializeDate(meta.defaultUserNameChanged),
  );
  appendText(document, element, 'MaintenanceHistoryDays', String(meta.maintenanceHistoryDays));
  appendText(document, element, 'Color', meta.color);
  appendText(document, element, 'MasterKeyChanged', serializeDate(meta.masterKeyChanged));
  appendText(document, element, 'MasterKeyChangeRec', String(meta.masterKeyChangeRec));
  appendText(document, element, 'MasterKeyChangeForce', String(meta.masterKeyChangeForce));
  const memory = appendElement(document, element, 'MemoryProtection');
  appendText(document, memory, 'ProtectTitle', xmlBoolean(meta.memoryProtection.title));
  appendText(document, memory, 'ProtectUserName', xmlBoolean(meta.memoryProtection.userName));
  appendText(document, memory, 'ProtectPassword', xmlBoolean(meta.memoryProtection.password));
  appendText(document, memory, 'ProtectURL', xmlBoolean(meta.memoryProtection.url));
  appendText(document, memory, 'ProtectNotes', xmlBoolean(meta.memoryProtection.notes));
  const customIcons = appendElement(document, element, 'CustomIcons');
  for (const icon of meta.customIcons) {
    const iconElement = appendElement(document, customIcons, 'Icon');
    appendText(document, iconElement, 'UUID', icon.uuid.toBase64());
    appendText(document, iconElement, 'Data', bytesToBase64(icon.data));
    if (icon.name !== undefined) {
      appendText(document, iconElement, 'Name', icon.name);
    }
    if (icon.lastModificationTime) {
      appendText(
        document,
        iconElement,
        'LastModificationTime',
        serializeDate(icon.lastModificationTime),
      );
    }
  }
  appendText(document, element, 'RecycleBinEnabled', xmlBoolean(meta.recycleBinEnabled));
  appendText(document, element, 'RecycleBinUUID', meta.recycleBinUuid.toBase64());
  appendText(document, element, 'RecycleBinChanged', serializeDate(meta.recycleBinChanged));
  appendText(document, element, 'EntryTemplatesGroup', meta.entryTemplatesGroup.toBase64());
  appendText(
    document,
    element,
    'EntryTemplatesGroupChanged',
    serializeDate(meta.entryTemplatesGroupChanged),
  );
  appendText(document, element, 'LastSelectedGroup', meta.lastSelectedGroup.toBase64());
  appendText(document, element, 'LastTopVisibleGroup', meta.lastTopVisibleGroup.toBase64());
  appendText(document, element, 'HistoryMaxItems', String(meta.historyMaxItems));
  appendText(document, element, 'HistoryMaxSize', String(meta.historyMaxSize));
  serializeCustomData(document, element, meta.customData, protectedStream);
  return element;
}

function parseGroup(
  element: XmlElement,
  protectedStream: ProtectedStream | undefined,
  binaries: KdbxBinary[],
): KdbxGroup {
  const group = new KdbxGroup(textChild(element, 'Name'));
  group.uuid = requiredUuidChild(element, 'UUID');
  group.notes = textChild(element, 'Notes');
  group.tags = textChild(element, 'Tags')
    .split(/[;,]/)
    .map(tag => tag.trim())
    .filter(Boolean);
  group.iconId = numberChild(element, 'IconID', 48);
  group.customIconUuid = uuidChild(element, 'CustomIconUUID');
  group.previousParentGroup = uuidChild(element, 'PreviousParentGroup');
  const times = child(element, 'Times');
  if (times) {
    group.times = parseTimes(times);
  }
  group.isExpanded = booleanChild(element, 'IsExpanded', true);
  group.defaultAutoTypeSequence = textChild(element, 'DefaultAutoTypeSequence');
  group.enableAutoType = nullableBooleanChild(element, 'EnableAutoType');
  group.enableSearching = nullableBooleanChild(element, 'EnableSearching');
  group.lastTopVisibleEntry = uuidChild(element, 'LastTopVisibleEntry');
  group.customData = parseCustomData(child(element, 'CustomData'), protectedStream);
  for (const item of childElements(element)) {
    if (item.tagName === 'Group') {
      group.groups.push(parseGroup(item, protectedStream, binaries));
    }
    if (item.tagName === 'Entry') {
      group.entries.push(parseEntry(item, protectedStream, binaries));
    }
  }
  return group;
}

function serializeGroup(
  document: XmlDocument,
  group: KdbxGroup,
  protectedStream: ProtectedStream,
  binaries: KdbxBinary[],
): XmlElement {
  const element = document.createElement('Group');
  appendText(document, element, 'UUID', group.uuid.toBase64());
  appendText(document, element, 'Name', group.name);
  appendText(document, element, 'Notes', group.notes);
  appendText(document, element, 'Tags', group.tags.join(';'));
  appendText(document, element, 'IconID', String(group.iconId));
  if (group.customIconUuid) {
    appendText(document, element, 'CustomIconUUID', group.customIconUuid.toBase64());
  }
  if (group.previousParentGroup) {
    appendText(document, element, 'PreviousParentGroup', group.previousParentGroup.toBase64());
  }
  element.appendChild(serializeTimes(document, group.times));
  appendText(document, element, 'IsExpanded', xmlBoolean(group.isExpanded));
  appendText(document, element, 'DefaultAutoTypeSequence', group.defaultAutoTypeSequence);
  appendText(document, element, 'EnableAutoType', xmlNullableBoolean(group.enableAutoType));
  appendText(document, element, 'EnableSearching', xmlNullableBoolean(group.enableSearching));
  appendText(
    document,
    element,
    'LastTopVisibleEntry',
    group.lastTopVisibleEntry?.toBase64() ?? new KdbxUuid(new Uint8Array(16)).toBase64(),
  );
  serializeCustomData(document, element, group.customData, protectedStream);
  for (const childGroup of group.groups) {
    element.appendChild(serializeGroup(document, childGroup, protectedStream, binaries));
  }
  for (const entry of group.entries) {
    element.appendChild(serializeEntry(document, entry, protectedStream, binaries));
  }
  return element;
}

function parseEntry(
  element: XmlElement,
  protectedStream: ProtectedStream | undefined,
  binaries: KdbxBinary[],
): KdbxEntry {
  const entry = new KdbxEntry();
  entry.fields.clear();
  entry.uuid = requiredUuidChild(element, 'UUID');
  entry.iconId = numberChild(element, 'IconID', 0);
  entry.customIconUuid = uuidChild(element, 'CustomIconUUID');
  entry.foregroundColor = textChild(element, 'ForegroundColor');
  entry.backgroundColor = textChild(element, 'BackgroundColor');
  entry.overrideUrl = textChild(element, 'OverrideURL');
  entry.qualityCheck = booleanChild(element, 'QualityCheck', true);
  entry.previousParentGroup = uuidChild(element, 'PreviousParentGroup');
  entry.tags = textChild(element, 'Tags')
    .split(/[;,]/)
    .map(tag => tag.trim())
    .filter(Boolean);
  const times = child(element, 'Times');
  if (times) {
    entry.times = parseTimes(times);
  }

  for (const item of childElements(element)) {
    if (item.tagName === 'String') {
      const keyElement = child(item, 'Key');
      const key = keyElement?.textContent ?? '';
      const valueElement = child(item, 'Value');
      if (!keyElement || !valueElement) {
        continue;
      }
      const isProtected = valueElement.getAttribute('Protected')?.toLowerCase() === 'true';
      let value = valueElement.textContent ?? '';
      if (isProtected) {
        if (!protectedStream) {
          throw new KdbxError('invalid-header', 'Protected XML value has no inner stream');
        }
        value = decodeUtf8(protectedStream.process(base64ToBytes(value)));
      }
      entry.fields.set(key, new KdbxValue(value, { protected: isProtected }));
    }
    if (item.tagName === 'Binary') {
      const keyElement = child(item, 'Key');
      const key = keyElement?.textContent ?? '';
      const valueElement = child(item, 'Value');
      const reference = valueElement?.getAttribute('Ref');
      if (
        !keyElement ||
        reference === null ||
        reference === undefined ||
        !/^\d+$/.test(reference)
      ) {
        continue;
      }
      const binary = binaries[Number(reference)];
      if (!binary) {
        throw new KdbxError('invalid-xml', `Invalid binary reference: ${reference}`);
      }
      entry.binaries.set(key, binary);
    }
  }
  entry.customData = parseCustomData(child(element, 'CustomData'), protectedStream);
  const autoType = child(element, 'AutoType');
  if (autoType) {
    entry.autoTypeEnabled = booleanChild(autoType, 'Enabled', true);
    entry.autoTypeObfuscation = numberChild(autoType, 'DataTransferObfuscation', 0);
    entry.defaultAutoTypeSequence = textChild(autoType, 'DefaultSequence');
    entry.autoTypeAssociations = children(autoType, 'Association').map(association => ({
      window: textChild(association, 'Window'),
      keystrokeSequence: textChild(association, 'KeystrokeSequence'),
    }));
  }
  const history = child(element, 'History');
  if (history) {
    entry.history = children(history, 'Entry').map(item =>
      parseEntry(item, protectedStream, binaries),
    );
  }
  return entry;
}

function serializeEntry(
  document: XmlDocument,
  entry: KdbxEntry,
  protectedStream: ProtectedStream,
  binaries: KdbxBinary[],
): XmlElement {
  const element = document.createElement('Entry');
  appendText(document, element, 'UUID', entry.uuid.toBase64());
  appendText(document, element, 'IconID', String(entry.iconId));
  if (entry.customIconUuid) {
    appendText(document, element, 'CustomIconUUID', entry.customIconUuid.toBase64());
  }
  appendText(document, element, 'ForegroundColor', entry.foregroundColor);
  appendText(document, element, 'BackgroundColor', entry.backgroundColor);
  appendText(document, element, 'OverrideURL', entry.overrideUrl);
  appendText(document, element, 'Tags', entry.tags.join(';'));
  appendText(document, element, 'QualityCheck', xmlBoolean(entry.qualityCheck));
  if (entry.previousParentGroup) {
    appendText(document, element, 'PreviousParentGroup', entry.previousParentGroup.toBase64());
  }
  element.appendChild(serializeTimes(document, entry.times));
  for (const [key, field] of entry.fields) {
    const stringElement = appendElement(document, element, 'String');
    appendText(document, stringElement, 'Key', key);
    const valueElement = appendText(
      document,
      stringElement,
      'Value',
      field.protected ? bytesToBase64(protectedStream.process(utf8(field.value))) : field.value,
    );
    if (field.protected) {
      valueElement.setAttribute('Protected', 'True');
    }
  }
  for (const [key, binary] of entry.binaries) {
    let reference = binaries.indexOf(binary);
    if (reference < 0) {
      reference = binaries.length;
      binaries.push(binary);
    }
    const binaryElement = appendElement(document, element, 'Binary');
    appendText(document, binaryElement, 'Key', key);
    const valueElement = appendElement(document, binaryElement, 'Value');
    valueElement.setAttribute('Ref', String(reference));
  }
  serializeCustomData(document, element, entry.customData, protectedStream);
  const autoType = appendElement(document, element, 'AutoType');
  appendText(document, autoType, 'Enabled', xmlBoolean(entry.autoTypeEnabled));
  appendText(document, autoType, 'DataTransferObfuscation', String(entry.autoTypeObfuscation));
  appendText(document, autoType, 'DefaultSequence', entry.defaultAutoTypeSequence);
  for (const association of entry.autoTypeAssociations) {
    const associationElement = appendElement(document, autoType, 'Association');
    appendText(document, associationElement, 'Window', association.window);
    appendText(document, associationElement, 'KeystrokeSequence', association.keystrokeSequence);
  }
  const history = appendElement(document, element, 'History');
  for (const historyEntry of entry.history) {
    history.appendChild(serializeEntry(document, historyEntry, protectedStream, binaries));
  }
  return element;
}

function parseCustomData(
  element: XmlElement | undefined,
  protectedStream: ProtectedStream | undefined,
): Map<string, KdbxCustomDataItem> {
  const result = new Map<string, KdbxCustomDataItem>();
  if (!element) {
    return result;
  }
  for (const item of children(element, 'Item')) {
    const keyElement = child(item, 'Key');
    const valueElement = child(item, 'Value');
    if (!keyElement || !valueElement) {
      continue;
    }
    const isProtected = valueElement.getAttribute('Protected')?.toLowerCase() === 'true';
    let value = valueElement.textContent ?? '';
    if (isProtected) {
      if (!protectedStream) {
        throw new KdbxError('invalid-header', 'Protected custom data has no inner stream');
      }
      value = decodeUtf8(protectedStream.process(base64ToBytes(value)));
    }
    result.set(keyElement.textContent ?? '', {
      value,
      protected: isProtected,
      lastModificationTime: dateChild(item, 'LastModificationTime'),
    });
  }
  return result;
}

function serializeCustomData(
  document: XmlDocument,
  parent: XmlElement,
  customData: Map<string, KdbxCustomDataItem>,
  protectedStream: ProtectedStream,
): void {
  if (customData.size === 0) {
    return;
  }
  const element = appendElement(document, parent, 'CustomData');
  for (const [key, item] of customData) {
    const itemElement = appendElement(document, element, 'Item');
    appendText(document, itemElement, 'Key', key);
    const valueElement = appendText(
      document,
      itemElement,
      'Value',
      item.protected ? bytesToBase64(protectedStream.process(utf8(item.value))) : item.value,
    );
    if (item.protected) {
      valueElement.setAttribute('Protected', 'True');
    }
    if (item.lastModificationTime) {
      appendText(
        document,
        itemElement,
        'LastModificationTime',
        serializeDate(item.lastModificationTime),
      );
    }
  }
}

function parseTimes(element: XmlElement): KdbxTimes {
  const times = new KdbxTimes();
  times.creationTime = dateChild(element, 'CreationTime') ?? times.creationTime;
  times.lastModificationTime =
    dateChild(element, 'LastModificationTime') ?? times.lastModificationTime;
  times.lastAccessTime = dateChild(element, 'LastAccessTime') ?? times.lastAccessTime;
  times.expiryTime = dateChild(element, 'ExpiryTime') ?? times.expiryTime;
  times.expires = booleanChild(element, 'Expires', false);
  times.usageCount = numberChild(element, 'UsageCount', 0);
  times.locationChanged = dateChild(element, 'LocationChanged') ?? times.locationChanged;
  return times;
}

function serializeTimes(document: XmlDocument, times: KdbxTimes): XmlElement {
  const element = document.createElement('Times');
  appendText(document, element, 'LastModificationTime', serializeDate(times.lastModificationTime));
  appendText(document, element, 'CreationTime', serializeDate(times.creationTime));
  appendText(document, element, 'LastAccessTime', serializeDate(times.lastAccessTime));
  appendText(document, element, 'ExpiryTime', serializeDate(times.expiryTime));
  appendText(document, element, 'Expires', xmlBoolean(times.expires));
  appendText(document, element, 'UsageCount', String(times.usageCount));
  appendText(document, element, 'LocationChanged', serializeDate(times.locationChanged));
  return element;
}

function parseDate(value: string): Date {
  if (/^\d{4}-/.test(value)) {
    const date = new Date(value);
    if (!Number.isNaN(date.getTime())) {
      return date;
    }
  }
  const bytes = base64ToBytes(value);
  if (bytes.length !== 8) {
    throw new KdbxError('invalid-xml', 'Invalid KDBX timestamp');
  }
  const seconds = new BinaryReader(bytes).readUint64() - secondsFromYearOneToUnixEpoch;
  const milliseconds = Number(seconds * 1000n);
  const date = new Date(milliseconds);
  if (Number.isNaN(date.getTime())) {
    throw new KdbxError('invalid-xml', 'Invalid KDBX timestamp');
  }
  return date;
}

function serializeDate(value: Date): string {
  const milliseconds = value.getTime();
  if (Number.isNaN(milliseconds)) {
    throw new KdbxError('invalid-xml', 'Cannot serialize invalid date');
  }
  const seconds = BigInt(Math.floor(milliseconds / 1000)) + secondsFromYearOneToUnixEpoch;
  return bytesToBase64(new BinaryWriter().writeUint64(seconds).toUint8Array());
}

function child(parent: XmlElement, name: string): XmlElement | undefined {
  return childElements(parent).find(element => element.tagName === name);
}

function children(parent: XmlElement, name: string): XmlElement[] {
  return childElements(parent).filter(element => element.tagName === name);
}

function childElements(parent: XmlElement): XmlElement[] {
  const result: XmlElement[] = [];
  for (let node = parent.firstChild; node; node = node.nextSibling) {
    if (node.nodeType === 1) {
      result.push(node as XmlElement);
    }
  }
  return result;
}

function textChild(parent: XmlElement, name: string): string {
  return child(parent, name)?.textContent ?? '';
}

function numberChild(parent: XmlElement, name: string, fallback: number): number {
  const value = Number.parseInt(textChild(parent, name), 10);
  return Number.isFinite(value) ? value : fallback;
}

function booleanChild(parent: XmlElement, name: string, fallback: boolean): boolean {
  const value = textChild(parent, name).toLowerCase();
  if (value === 'true' || value === '1') {
    return true;
  }
  if (value === 'false' || value === '0') {
    return false;
  }
  return fallback;
}

function nullableBooleanChild(parent: XmlElement, name: string): boolean | null {
  const value = textChild(parent, name).toLowerCase();
  if (value === 'true' || value === '1') {
    return true;
  }
  if (value === 'false' || value === '0') {
    return false;
  }
  return null;
}

function uuidChild(parent: XmlElement, name: string): KdbxUuid | undefined {
  const value = textChild(parent, name);
  return value ? KdbxUuid.fromBase64(value) : undefined;
}

function requiredUuidChild(parent: XmlElement, name: string): KdbxUuid {
  const value = uuidChild(parent, name);
  if (!value) {
    throw new KdbxError('invalid-xml', `Missing ${name}`);
  }
  return value;
}

function dateChild(parent: XmlElement, name: string): Date | undefined {
  const value = textChild(parent, name);
  return value ? parseDate(value) : undefined;
}

function appendElement(document: XmlDocument, parent: XmlNode, name: string): XmlElement {
  const element = document.createElement(name);
  parent.appendChild(element);
  return element;
}

function appendText(
  document: XmlDocument,
  parent: XmlNode,
  name: string,
  value: string,
): XmlElement {
  for (const character of value) {
    const codePoint = character.codePointAt(0)!;
    const valid =
      codePoint === 0x09 ||
      codePoint === 0x0a ||
      codePoint === 0x0d ||
      (codePoint >= 0x20 && codePoint <= 0xd7ff) ||
      (codePoint >= 0xe000 && codePoint <= 0xfffd) ||
      (codePoint >= 0x10000 && codePoint <= 0x10ffff);
    if (!valid) {
      throw new KdbxError('invalid-xml', `Value for ${name} contains an invalid XML character`);
    }
  }
  const element = appendElement(document, parent, name);
  element.appendChild(document.createTextNode(value));
  return element;
}

function xmlBoolean(value: boolean): string {
  return value ? 'True' : 'False';
}

function xmlNullableBoolean(value: boolean | null): string {
  return value === null ? 'null' : xmlBoolean(value);
}
