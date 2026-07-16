import { DOMParser } from '@xmldom/xmldom';
import { argon2d, argon2id } from 'hash-wasm';
import {
  asArrayBuffer,
  base64ToBytes,
  BinaryWriter,
  bytesToHex,
  concatBytes,
  equalBytes,
  hexToBytes,
  utf8,
} from './binary.ts';
import { KdfId } from './constants.ts';
import { KdbxError } from './errors.ts';
import type { BinaryReader } from './binary.ts';
import type { VariantDictionary } from './variant-dictionary.ts';

export interface KdbxCredentialOptions {
  password?: string;
  keyFile?: ArrayBuffer | Uint8Array;
}

export interface KdbxKdfLimits {
  maxAesRounds: number;
  maxArgon2Iterations: number;
  maxArgon2MemoryBytes: number;
  maxArgon2Parallelism: number;
}

export const defaultKdbxKdfLimits: KdbxKdfLimits = {
  maxAesRounds: 1_000_000,
  maxArgon2Iterations: 100,
  maxArgon2MemoryBytes: 256 * 1024 * 1024,
  maxArgon2Parallelism: 16,
};

export class KdbxCredentials {
  readonly password?: string;
  readonly keyFile?: Uint8Array;

  constructor(options: KdbxCredentialOptions = {}) {
    this.password = options.password;
    this.keyFile = options.keyFile
      ? options.keyFile instanceof Uint8Array
        ? options.keyFile.slice()
        : new Uint8Array(options.keyFile)
      : undefined;
  }

  async getCompositeKey(): Promise<Uint8Array> {
    const components: Uint8Array[] = [];
    if (this.password !== undefined) {
      components.push(await sha256(utf8(this.password)));
    }
    if (this.keyFile !== undefined) {
      components.push(await readKeyFile(this.keyFile));
    }
    return sha256(concatBytes(...components));
  }
}

export async function sha256(value: Uint8Array): Promise<Uint8Array> {
  return new Uint8Array(await crypto.subtle.digest('SHA-256', asArrayBuffer(value)));
}

export async function sha512(value: Uint8Array): Promise<Uint8Array> {
  return new Uint8Array(await crypto.subtle.digest('SHA-512', asArrayBuffer(value)));
}

export async function hmacSha256(key: Uint8Array, value: Uint8Array): Promise<Uint8Array> {
  const cryptoKey = await crypto.subtle.importKey(
    'raw',
    asArrayBuffer(key),
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    ['sign'],
  );
  return new Uint8Array(await crypto.subtle.sign('HMAC', cryptoKey, asArrayBuffer(value)));
}

export async function deriveKdfKey(
  compositeKey: Uint8Array,
  parameters: VariantDictionary,
  limits: KdbxKdfLimits = defaultKdbxKdfLimits,
): Promise<Uint8Array> {
  const id = requireBytes(parameters, '$UUID');
  const idHex = bytesToHex(id);
  if (idHex === KdfId.Aes) {
    const seed = requireBytes(parameters, 'S');
    const rounds = toSafeNumber(requireBigInt(parameters, 'R'), 'AES-KDF rounds');
    if (seed.length !== 32 || rounds < 1) {
      throw new KdbxError('invalid-header', 'Invalid AES-KDF seed');
    }
    enforceLimit(rounds, limits.maxAesRounds, 'AES-KDF rounds');
    return aesKdf(compositeKey, seed, rounds);
  }
  if (idHex !== KdfId.Argon2d && idHex !== KdfId.Argon2id) {
    throw new KdbxError('unsupported-kdf', `Unsupported KDBX KDF: ${idHex}`);
  }

  const version = requireNumber(parameters, 'V');
  if (version !== 0x13) {
    throw new KdbxError('unsupported-kdf', `Unsupported Argon2 version: ${version}`);
  }
  if (parameters.has('A')) {
    throw new KdbxError('unsupported-kdf', 'Argon2 associated data is not supported');
  }
  const salt = requireBytes(parameters, 'S');
  const iterations = toSafeNumber(requireBigInt(parameters, 'I'), 'Argon2 iterations');
  const memoryBytes = toSafeNumber(requireBigInt(parameters, 'M'), 'Argon2 memory');
  const parallelism = requireNumber(parameters, 'P');
  if (salt.length < 8 || iterations < 1 || iterations > 0xffffffff) {
    throw new KdbxError('invalid-header', 'Invalid Argon2 salt or iteration count');
  }
  if (
    memoryBytes < 8192 ||
    memoryBytes > 0x7fffffff ||
    memoryBytes % 1024 !== 0 ||
    parallelism < 1 ||
    parallelism > 0x00ffffff
  ) {
    throw new KdbxError('invalid-header', 'Argon2 memory must be a multiple of 1024 bytes');
  }
  enforceLimit(iterations, limits.maxArgon2Iterations, 'Argon2 iterations');
  enforceLimit(memoryBytes, limits.maxArgon2MemoryBytes, 'Argon2 memory');
  enforceLimit(parallelism, limits.maxArgon2Parallelism, 'Argon2 parallelism');
  const options = {
    password: compositeKey,
    salt,
    secret: optionalBytes(parameters, 'K'),
    iterations,
    parallelism,
    memorySize: memoryBytes / 1024,
    hashLength: 32,
    outputType: 'binary' as const,
  };
  const result = idHex === KdfId.Argon2id ? await argon2id(options) : await argon2d(options);
  if (!(result instanceof Uint8Array)) {
    throw new KdbxError('unsupported-kdf', 'Argon2 returned an invalid result');
  }
  return result;
}

export async function deriveKeys(
  masterSeed: Uint8Array,
  transformedKey: Uint8Array,
): Promise<{ cipherKey: Uint8Array; hmacKey: Uint8Array }> {
  const material = concatBytes(masterSeed, transformedKey);
  return {
    cipherKey: await sha256(material),
    hmacKey: await sha512(concatBytes(material, Uint8Array.of(1))),
  };
}

export async function deriveBlockHmacKey(baseKey: Uint8Array, index: bigint): Promise<Uint8Array> {
  return sha512(new BinaryWriter().writeUint64(index).writeBytes(baseKey).toUint8Array());
}

export async function aesCbcEncrypt(
  value: Uint8Array,
  key: Uint8Array,
  iv: Uint8Array,
): Promise<Uint8Array> {
  const cryptoKey = await importAesKey(key, ['encrypt']);
  return new Uint8Array(
    await crypto.subtle.encrypt(
      { name: 'AES-CBC', iv: asArrayBuffer(iv) },
      cryptoKey,
      asArrayBuffer(value),
    ),
  );
}

export async function aesCbcDecrypt(
  value: Uint8Array,
  key: Uint8Array,
  iv: Uint8Array,
): Promise<Uint8Array> {
  try {
    const cryptoKey = await importAesKey(key, ['decrypt']);
    return new Uint8Array(
      await crypto.subtle.decrypt(
        { name: 'AES-CBC', iv: asArrayBuffer(iv) },
        cryptoKey,
        asArrayBuffer(value),
      ),
    );
  } catch (error) {
    throw new KdbxError('corrupt-data', 'Unable to decrypt KDBX payload', { cause: error });
  }
}

export async function writeHmacBlocks(
  value: Uint8Array,
  baseKey: Uint8Array,
  blockSize = 1024 * 1024,
): Promise<Uint8Array> {
  const writer = new BinaryWriter();
  let index = 0n;
  for (let offset = 0; offset < value.length; offset += blockSize) {
    const block = value.subarray(offset, offset + blockSize);
    const size = new BinaryWriter().writeUint32(block.length).toUint8Array();
    const indexBytes = new BinaryWriter().writeUint64(index).toUint8Array();
    const key = await deriveBlockHmacKey(baseKey, index);
    writer.writeBytes(await hmacSha256(key, concatBytes(indexBytes, size, block)));
    writer.writeBytes(size).writeBytes(block);
    index += 1n;
  }
  const size = new Uint8Array(4);
  const indexBytes = new BinaryWriter().writeUint64(index).toUint8Array();
  const key = await deriveBlockHmacKey(baseKey, index);
  writer.writeBytes(await hmacSha256(key, concatBytes(indexBytes, size))).writeBytes(size);
  return writer.toUint8Array();
}

export async function readHmacBlocks(
  reader: BinaryReader,
  baseKey: Uint8Array,
): Promise<Uint8Array> {
  const blocks: Uint8Array[] = [];
  let index = 0n;
  while (true) {
    const expected = reader.readBytes(32);
    const sizeBytes = reader.readBytes(4);
    const size = new DataView(sizeBytes.buffer, sizeBytes.byteOffset, 4).getUint32(0, true);
    const block = reader.readBytes(size);
    const indexBytes = new BinaryWriter().writeUint64(index).toUint8Array();
    const key = await deriveBlockHmacKey(baseKey, index);
    const actual = await hmacSha256(key, concatBytes(indexBytes, sizeBytes, block));
    if (!equalBytes(expected, actual)) {
      throw new KdbxError('corrupt-data', `Invalid payload HMAC at block ${index}`);
    }
    if (size === 0) {
      if (reader.remaining !== 0) {
        throw new KdbxError('corrupt-data', 'Data follows KDBX payload terminator');
      }
      return concatBytes(...blocks);
    }
    blocks.push(block);
    index += 1n;
  }
}

export class ChaCha20Stream {
  private readonly state: Uint32Array;
  private block = new Uint8Array(0);
  private blockOffset = 0;

  constructor(key: Uint8Array, nonce: Uint8Array, counter = 0) {
    if (key.length !== 32 || nonce.length !== 12) {
      throw new KdbxError('invalid-header', 'ChaCha20 requires a 32-byte key and 12-byte nonce');
    }
    this.state = new Uint32Array(16);
    this.state.set([0x61707865, 0x3320646e, 0x79622d32, 0x6b206574]);
    for (let index = 0; index < 8; index += 1) {
      this.state[index + 4] = readWord(key, index * 4);
    }
    this.state[12] = counter;
    this.state[13] = readWord(nonce, 0);
    this.state[14] = readWord(nonce, 4);
    this.state[15] = readWord(nonce, 8);
  }

  process(value: Uint8Array): Uint8Array {
    const result = new Uint8Array(value.length);
    for (let index = 0; index < value.length; index += 1) {
      if (this.blockOffset >= this.block.length) {
        this.nextBlock();
      }
      result[index] = value[index] ^ this.block[this.blockOffset++];
    }
    return result;
  }

  private nextBlock(): void {
    const working = this.state.slice();
    for (let round = 0; round < 10; round += 1) {
      chachaQuarterRound(working, 0, 4, 8, 12);
      chachaQuarterRound(working, 1, 5, 9, 13);
      chachaQuarterRound(working, 2, 6, 10, 14);
      chachaQuarterRound(working, 3, 7, 11, 15);
      chachaQuarterRound(working, 0, 5, 10, 15);
      chachaQuarterRound(working, 1, 6, 11, 12);
      chachaQuarterRound(working, 2, 7, 8, 13);
      chachaQuarterRound(working, 3, 4, 9, 14);
    }
    this.block = new Uint8Array(64);
    const view = new DataView(this.block.buffer);
    for (let index = 0; index < 16; index += 1) {
      view.setUint32(index * 4, (working[index] + this.state[index]) >>> 0, true);
    }
    this.blockOffset = 0;
    this.state[12] = (this.state[12] + 1) >>> 0;
    if (this.state[12] === 0) {
      throw new KdbxError('corrupt-data', 'ChaCha20 counter exhausted');
    }
  }
}

export class Salsa20Stream {
  private readonly state = new Uint32Array(16);
  private block = new Uint8Array(0);
  private blockOffset = 0;

  constructor(key: Uint8Array, nonce: Uint8Array) {
    if (key.length !== 32 || nonce.length !== 8) {
      throw new KdbxError('invalid-header', 'Salsa20 requires a 32-byte key and 8-byte nonce');
    }
    this.state[0] = 0x61707865;
    this.state[5] = 0x3320646e;
    this.state[10] = 0x79622d32;
    this.state[15] = 0x6b206574;
    for (let index = 0; index < 4; index += 1) {
      this.state[index + 1] = readWord(key, index * 4);
      this.state[index + 11] = readWord(key, (index + 4) * 4);
    }
    this.state[6] = readWord(nonce, 0);
    this.state[7] = readWord(nonce, 4);
  }

  process(value: Uint8Array): Uint8Array {
    const result = new Uint8Array(value.length);
    for (let index = 0; index < value.length; index += 1) {
      if (this.blockOffset >= this.block.length) {
        this.nextBlock();
      }
      result[index] = value[index] ^ this.block[this.blockOffset++];
    }
    return result;
  }

  private nextBlock(): void {
    const x = this.state.slice();
    for (let round = 0; round < 10; round += 1) {
      x[4] ^= rotateLeft((x[0] + x[12]) >>> 0, 7);
      x[8] ^= rotateLeft((x[4] + x[0]) >>> 0, 9);
      x[12] ^= rotateLeft((x[8] + x[4]) >>> 0, 13);
      x[0] ^= rotateLeft((x[12] + x[8]) >>> 0, 18);
      x[9] ^= rotateLeft((x[5] + x[1]) >>> 0, 7);
      x[13] ^= rotateLeft((x[9] + x[5]) >>> 0, 9);
      x[1] ^= rotateLeft((x[13] + x[9]) >>> 0, 13);
      x[5] ^= rotateLeft((x[1] + x[13]) >>> 0, 18);
      x[14] ^= rotateLeft((x[10] + x[6]) >>> 0, 7);
      x[2] ^= rotateLeft((x[14] + x[10]) >>> 0, 9);
      x[6] ^= rotateLeft((x[2] + x[14]) >>> 0, 13);
      x[10] ^= rotateLeft((x[6] + x[2]) >>> 0, 18);
      x[3] ^= rotateLeft((x[15] + x[11]) >>> 0, 7);
      x[7] ^= rotateLeft((x[3] + x[15]) >>> 0, 9);
      x[11] ^= rotateLeft((x[7] + x[3]) >>> 0, 13);
      x[15] ^= rotateLeft((x[11] + x[7]) >>> 0, 18);
      x[1] ^= rotateLeft((x[0] + x[3]) >>> 0, 7);
      x[2] ^= rotateLeft((x[1] + x[0]) >>> 0, 9);
      x[3] ^= rotateLeft((x[2] + x[1]) >>> 0, 13);
      x[0] ^= rotateLeft((x[3] + x[2]) >>> 0, 18);
      x[6] ^= rotateLeft((x[5] + x[4]) >>> 0, 7);
      x[7] ^= rotateLeft((x[6] + x[5]) >>> 0, 9);
      x[4] ^= rotateLeft((x[7] + x[6]) >>> 0, 13);
      x[5] ^= rotateLeft((x[4] + x[7]) >>> 0, 18);
      x[11] ^= rotateLeft((x[10] + x[9]) >>> 0, 7);
      x[8] ^= rotateLeft((x[11] + x[10]) >>> 0, 9);
      x[9] ^= rotateLeft((x[8] + x[11]) >>> 0, 13);
      x[10] ^= rotateLeft((x[9] + x[8]) >>> 0, 18);
      x[12] ^= rotateLeft((x[15] + x[14]) >>> 0, 7);
      x[13] ^= rotateLeft((x[12] + x[15]) >>> 0, 9);
      x[14] ^= rotateLeft((x[13] + x[12]) >>> 0, 13);
      x[15] ^= rotateLeft((x[14] + x[13]) >>> 0, 18);
    }
    this.block = new Uint8Array(64);
    const view = new DataView(this.block.buffer);
    for (let index = 0; index < 16; index += 1) {
      view.setUint32(index * 4, (x[index] + this.state[index]) >>> 0, true);
    }
    this.blockOffset = 0;
    this.state[8] = (this.state[8] + 1) >>> 0;
    if (this.state[8] === 0) {
      this.state[9] = (this.state[9] + 1) >>> 0;
    }
  }
}

async function aesKdf(value: Uint8Array, seed: Uint8Array, rounds: number): Promise<Uint8Array> {
  if (value.length !== 32) {
    throw new KdbxError('invalid-header', 'Invalid AES-KDF input');
  }
  const key = await importAesKey(seed, ['encrypt']);
  const iv = new Uint8Array(16);
  const transformed = value.slice();
  for (let round = 0; round < rounds; round += 1) {
    for (let offset = 0; offset < 32; offset += 16) {
      const encrypted = await crypto.subtle.encrypt(
        { name: 'AES-CBC', iv },
        key,
        asArrayBuffer(transformed.subarray(offset, offset + 16)),
      );
      transformed.set(new Uint8Array(encrypted, 0, 16), offset);
    }
  }
  return sha256(transformed);
}

async function importAesKey(key: Uint8Array, usages: KeyUsage[]): Promise<CryptoKey> {
  if (key.length !== 32) {
    throw new KdbxError('invalid-header', 'AES-256 requires a 32-byte key');
  }
  return crypto.subtle.importKey('raw', asArrayBuffer(key), 'AES-CBC', false, usages);
}

async function readKeyFile(value: Uint8Array): Promise<Uint8Array> {
  if (value.length === 32) {
    return value.slice();
  }
  const exactText = new TextDecoder().decode(value);
  if (value.length === 64 && /^[\da-f]{64}$/i.test(exactText)) {
    return hexToBytes(exactText);
  }
  const text = exactText.trim();
  if (text.startsWith('<?xml') || text.startsWith('<KeyFile')) {
    const document = new DOMParser({ onError: () => undefined }).parseFromString(
      text,
      'application/xml',
    );
    const root = document.documentElement;
    const version = root?.getElementsByTagName('Version').item(0)?.textContent?.trim();
    const dataElement = root?.getElementsByTagName('Data').item(0);
    if (root?.tagName !== 'KeyFile' || !version || !dataElement) {
      throw new KdbxError('invalid-key-file', 'Invalid XML key file');
    }
    const encodedData = (dataElement.textContent ?? '').replace(/\s/g, '');
    const hashValue = dataElement.getAttribute('Hash');
    let data: Uint8Array;
    if (version.startsWith('1.')) {
      if (hashValue !== null) {
        throw new KdbxError('invalid-key-file', 'Key-file v1 must not contain a data hash');
      }
      data = base64ToBytes(encodedData);
    } else if (version.startsWith('2.')) {
      if (!hashValue || !/^[\da-f]{8}$/i.test(hashValue)) {
        throw new KdbxError('invalid-key-file', 'Key-file v2 data hash is missing');
      }
      data = hexToBytes(encodedData);
    } else {
      throw new KdbxError('invalid-key-file', `Unsupported key-file version: ${version}`);
    }
    if (data.length !== 32) {
      throw new KdbxError('invalid-key-file', 'Key file data must be 32 bytes');
    }
    if (hashValue) {
      const hash = await sha256(data);
      if (bytesToHex(hash.subarray(0, 4)) !== hashValue.toLowerCase()) {
        throw new KdbxError('invalid-key-file', 'Key file hash does not match');
      }
    }
    return data;
  }
  return sha256(value);
}

function requireBytes(parameters: VariantDictionary, key: string): Uint8Array {
  const value = parameters.get(key);
  if (!(value instanceof Uint8Array)) {
    throw new KdbxError('invalid-header', `Missing byte-array KDF parameter: ${key}`);
  }
  return value;
}

function optionalBytes(parameters: VariantDictionary, key: string): Uint8Array | undefined {
  if (!parameters.has(key)) {
    return undefined;
  }
  return requireBytes(parameters, key);
}

function requireBigInt(parameters: VariantDictionary, key: string): bigint {
  const value = parameters.get(key);
  if (typeof value !== 'bigint') {
    throw new KdbxError('invalid-header', `Missing uint64 KDF parameter: ${key}`);
  }
  return value;
}

function requireNumber(parameters: VariantDictionary, key: string): number {
  const value = parameters.get(key);
  if (typeof value !== 'number') {
    throw new KdbxError('invalid-header', `Missing uint32 KDF parameter: ${key}`);
  }
  return value;
}

function toSafeNumber(value: bigint, label: string): number {
  if (value < 0n || value > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new KdbxError('invalid-header', `${label} is outside the supported range`);
  }
  return Number(value);
}

function enforceLimit(value: number, maximum: number, label: string): void {
  if (!Number.isSafeInteger(maximum) || maximum < 1 || value > maximum) {
    throw new KdbxError('invalid-header', `${label} exceeds the configured resource limit`);
  }
}

function readWord(value: Uint8Array, offset: number): number {
  return new DataView(value.buffer, value.byteOffset + offset, 4).getUint32(0, true);
}

function rotateLeft(value: number, count: number): number {
  return ((value << count) | (value >>> (32 - count))) >>> 0;
}

function chachaQuarterRound(state: Uint32Array, a: number, b: number, c: number, d: number): void {
  state[a] = (state[a] + state[b]) >>> 0;
  state[d] = rotateLeft(state[d] ^ state[a], 16);
  state[c] = (state[c] + state[d]) >>> 0;
  state[b] = rotateLeft(state[b] ^ state[c], 12);
  state[a] = (state[a] + state[b]) >>> 0;
  state[d] = rotateLeft(state[d] ^ state[a], 8);
  state[c] = (state[c] + state[d]) >>> 0;
  state[b] = rotateLeft(state[b] ^ state[c], 7);
}
