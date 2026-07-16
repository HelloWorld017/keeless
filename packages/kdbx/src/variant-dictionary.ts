import { BinaryReader, BinaryWriter, utf8 } from './binary.ts';
import { KdbxError } from './errors.ts';

export type VariantValue = Uint8Array | string | number | bigint | boolean;
export type VariantDictionary = Map<string, VariantValue>;

const VariantType = {
  UInt32: 0x04,
  UInt64: 0x05,
  Bool: 0x08,
  Int32: 0x0c,
  Int64: 0x0d,
  String: 0x18,
  Bytes: 0x42,
} as const;

export function readVariantDictionary(value: Uint8Array): VariantDictionary {
  const reader = new BinaryReader(value);
  const version = reader.readUint16();
  if ((version & 0xff00) > 0x0100) {
    throw new KdbxError('unsupported-version', 'Unsupported variant dictionary version');
  }

  const result: VariantDictionary = new Map();
  while (true) {
    const type = reader.readUint8();
    if (type === 0) {
      break;
    }
    const nameLength = reader.readInt32();
    if (nameLength < 0) {
      throw new KdbxError('corrupt-data', 'Negative variant name length');
    }
    const name = reader.readString(nameLength);
    const valueLength = reader.readInt32();
    if (valueLength < 0) {
      throw new KdbxError('corrupt-data', 'Negative variant value length');
    }
    const bytes = reader.readBytes(valueLength);
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);

    let item: VariantValue;
    switch (type) {
      case VariantType.UInt32:
        requireLength(bytes, 4);
        item = view.getUint32(0, true);
        break;
      case VariantType.UInt64:
        requireLength(bytes, 8);
        item = view.getBigUint64(0, true);
        break;
      case VariantType.Bool:
        requireLength(bytes, 1);
        item = bytes[0] !== 0;
        break;
      case VariantType.Int32:
        requireLength(bytes, 4);
        item = view.getInt32(0, true);
        break;
      case VariantType.Int64:
        requireLength(bytes, 8);
        item = view.getBigInt64(0, true);
        break;
      case VariantType.String:
        item = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
        break;
      case VariantType.Bytes:
        item = bytes;
        break;
      default:
        throw new KdbxError('corrupt-data', `Unsupported variant type: ${type}`);
    }
    result.set(name, item);
  }
  if (reader.remaining !== 0) {
    throw new KdbxError('corrupt-data', 'Data follows variant dictionary terminator');
  }
  return result;
}

export function writeVariantDictionary(dictionary: VariantDictionary): Uint8Array {
  const writer = new BinaryWriter().writeUint16(0x0100);
  for (const [name, value] of dictionary) {
    const nameBytes = utf8(name);
    const encoded = encodeVariant(value);
    writer
      .writeUint8(encoded.type)
      .writeInt32(nameBytes.length)
      .writeBytes(nameBytes)
      .writeInt32(encoded.bytes.length)
      .writeBytes(encoded.bytes);
  }
  return writer.writeUint8(0).toUint8Array();
}

function encodeVariant(value: VariantValue): { type: number; bytes: Uint8Array } {
  if (value instanceof Uint8Array) {
    return { type: VariantType.Bytes, bytes: value };
  }
  if (typeof value === 'string') {
    return { type: VariantType.String, bytes: utf8(value) };
  }
  if (typeof value === 'boolean') {
    return { type: VariantType.Bool, bytes: Uint8Array.of(value ? 1 : 0) };
  }
  const writer = new BinaryWriter();
  if (typeof value === 'bigint') {
    return { type: VariantType.UInt64, bytes: writer.writeUint64(value).toUint8Array() };
  }
  if (!Number.isInteger(value) || value < 0 || value > 0xffffffff) {
    throw new KdbxError('invalid-header', 'Variant number must be a uint32');
  }
  return { type: VariantType.UInt32, bytes: writer.writeUint32(value).toUint8Array() };
}

function requireLength(value: Uint8Array, expected: number): void {
  if (value.length !== expected) {
    throw new KdbxError('corrupt-data', 'Invalid variant value length');
  }
}
