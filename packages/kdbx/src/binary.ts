import { KdbxError } from './errors.ts';

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder('utf-8', { fatal: true });

export class BinaryReader {
  readonly bytes: Uint8Array;
  offset = 0;

  constructor(value: ArrayBuffer | Uint8Array) {
    this.bytes = value instanceof Uint8Array ? value : new Uint8Array(value);
  }

  get remaining(): number {
    return this.bytes.length - this.offset;
  }

  readBytes(length: number): Uint8Array {
    if (!Number.isSafeInteger(length) || length < 0 || length > this.remaining) {
      throw new KdbxError('corrupt-data', 'Unexpected end of KDBX data');
    }
    const result = this.bytes.slice(this.offset, this.offset + length);
    this.offset += length;
    return result;
  }

  readUint8(): number {
    return this.readBytes(1)[0];
  }

  readUint16(): number {
    const bytes = this.readBytes(2);
    return new DataView(bytes.buffer, bytes.byteOffset, 2).getUint16(0, true);
  }

  readUint32(): number {
    const bytes = this.readBytes(4);
    return new DataView(bytes.buffer, bytes.byteOffset, 4).getUint32(0, true);
  }

  readInt32(): number {
    const bytes = this.readBytes(4);
    return new DataView(bytes.buffer, bytes.byteOffset, 4).getInt32(0, true);
  }

  readUint64(): bigint {
    const bytes = this.readBytes(8);
    return new DataView(bytes.buffer, bytes.byteOffset, 8).getBigUint64(0, true);
  }

  readString(length: number): string {
    try {
      return textDecoder.decode(this.readBytes(length));
    } catch (error) {
      throw new KdbxError('corrupt-data', 'KDBX contains invalid UTF-8', { cause: error });
    }
  }
}

export class BinaryWriter {
  private readonly chunks: Uint8Array[] = [];
  private totalLength = 0;

  writeBytes(value: ArrayBuffer | Uint8Array): this {
    const bytes = value instanceof Uint8Array ? value : new Uint8Array(value);
    this.chunks.push(bytes);
    this.totalLength += bytes.length;
    return this;
  }

  writeUint8(value: number): this {
    return this.writeBytes(Uint8Array.of(value));
  }

  writeUint16(value: number): this {
    const bytes = new Uint8Array(2);
    new DataView(bytes.buffer).setUint16(0, value, true);
    return this.writeBytes(bytes);
  }

  writeUint32(value: number): this {
    const bytes = new Uint8Array(4);
    new DataView(bytes.buffer).setUint32(0, value, true);
    return this.writeBytes(bytes);
  }

  writeInt32(value: number): this {
    const bytes = new Uint8Array(4);
    new DataView(bytes.buffer).setInt32(0, value, true);
    return this.writeBytes(bytes);
  }

  writeUint64(value: bigint): this {
    const bytes = new Uint8Array(8);
    new DataView(bytes.buffer).setBigUint64(0, value, true);
    return this.writeBytes(bytes);
  }

  writeString(value: string): this {
    return this.writeBytes(textEncoder.encode(value));
  }

  toUint8Array(): Uint8Array {
    return concatBytes(...this.chunks);
  }

  get length(): number {
    return this.totalLength;
  }
}

export function concatBytes(...chunks: Uint8Array[]): Uint8Array {
  const result = new Uint8Array(chunks.reduce((sum, chunk) => sum + chunk.length, 0));
  let offset = 0;
  for (const chunk of chunks) {
    result.set(chunk, offset);
    offset += chunk.length;
  }
  return result;
}

export function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('');
}

export function hexToBytes(value: string): Uint8Array {
  if (value.length % 2 !== 0 || !/^[\da-f]*$/i.test(value)) {
    throw new KdbxError('corrupt-data', 'Invalid hexadecimal value');
  }
  return Uint8Array.from(value.match(/.{2}/g) ?? [], byte => Number.parseInt(byte, 16));
}

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary);
}

export function base64ToBytes(value: string): Uint8Array {
  try {
    const binary = atob(value);
    return Uint8Array.from(binary, character => character.charCodeAt(0));
  } catch (error) {
    throw new KdbxError('corrupt-data', 'Invalid Base64 value', { cause: error });
  }
}

export function equalBytes(left: Uint8Array, right: Uint8Array): boolean {
  if (left.length !== right.length) {
    return false;
  }
  let difference = 0;
  for (let index = 0; index < left.length; index += 1) {
    difference |= left[index] ^ right[index];
  }
  return difference === 0;
}

export function utf8(value: string): Uint8Array {
  return textEncoder.encode(value);
}

export function decodeUtf8(value: Uint8Array): string {
  try {
    return textDecoder.decode(value);
  } catch (error) {
    throw new KdbxError('invalid-xml', 'KDBX XML contains invalid UTF-8', { cause: error });
  }
}

export function randomBytes(length: number): Uint8Array {
  return crypto.getRandomValues(new Uint8Array(length));
}

export function asArrayBuffer(value: Uint8Array): ArrayBuffer {
  return value.slice().buffer;
}
