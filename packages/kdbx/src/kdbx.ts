import { gunzipSync, gzipSync } from 'fflate';
import {
  BinaryReader,
  BinaryWriter,
  bytesToHex,
  concatBytes,
  decodeUtf8,
  equalBytes,
  hexToBytes,
  randomBytes,
  utf8,
} from './binary.ts';
import {
  CipherId,
  Compression,
  InnerStream,
  KDBX_SIGNATURE_1,
  KDBX_SIGNATURE_2,
  KDBX_VERSION_4_1,
} from './constants.ts';
import {
  aesCbcDecrypt,
  aesCbcEncrypt,
  ChaCha20Stream,
  defaultKdbxKdfLimits,
  deriveBlockHmacKey,
  deriveKdfKey,
  deriveKeys,
  hmacSha256,
  KdbxCredentials,
  type KdbxKdfLimits,
  readHmacBlocks,
  Salsa20Stream,
  sha256,
  sha512,
  writeHmacBlocks,
} from './crypto.ts';
import { KdbxError } from './errors.ts';
import { KdbxBinary, KdbxDatabase } from './model.ts';
import {
  readVariantDictionary,
  type VariantDictionary,
  writeVariantDictionary,
} from './variant-dictionary.ts';
import { parseDatabaseXml, serializeDatabaseXml, type ProtectedStream } from './xml.ts';

interface OuterHeader {
  bytes: Uint8Array;
  version: number;
  cipher: string;
  compression: number;
  masterSeed: Uint8Array;
  iv: Uint8Array;
  kdfParameters: VariantDictionary;
  publicCustomData: VariantDictionary;
}

interface InnerHeader {
  streamId: number;
  streamKey: Uint8Array;
  binaries: KdbxBinary[];
  xml: string;
}

export interface SaveKdbxOptions {
  cipher?: (typeof CipherId)[keyof typeof CipherId];
  compression?: (typeof Compression)[keyof typeof Compression];
}

export const Kdbx = {
  create(name = ''): KdbxDatabase {
    return KdbxDatabase.create(name);
  },

  load(
    value: ArrayBuffer | Uint8Array,
    credentials: KdbxCredentials | string,
  ): Promise<KdbxDatabase> {
    return loadKdbx(value, normalizeCredentials(credentials));
  },

  save(
    database: KdbxDatabase,
    credentials: KdbxCredentials | string,
    options?: SaveKdbxOptions,
  ): Promise<Uint8Array> {
    return saveKdbx(database, normalizeCredentials(credentials), options);
  },
} as const;

export async function loadKdbx(
  value: ArrayBuffer | Uint8Array,
  credentials: KdbxCredentials,
): Promise<KdbxDatabase> {
  const reader = new BinaryReader(value);
  const header = readOuterHeader(reader);
  const storedHash = reader.readBytes(32);
  const actualHash = await sha256(header.bytes);
  if (!equalBytes(storedHash, actualHash)) {
    throw new KdbxError('corrupt-data', 'KDBX header hash does not match');
  }

  const compositeKey = await credentials.getCompositeKey();
  const transformedKey = await deriveKdfKey(compositeKey, header.kdfParameters);
  const { cipherKey, hmacKey } = await deriveKeys(header.masterSeed, transformedKey);
  const storedHmac = reader.readBytes(32);
  const headerHmacKey = await deriveBlockHmacKey(hmacKey, 0xffffffffffffffffn);
  const actualHmac = await hmacSha256(headerHmacKey, header.bytes);
  if (!equalBytes(storedHmac, actualHmac)) {
    throw new KdbxError('invalid-credentials', 'Invalid KDBX credentials');
  }

  const encrypted = await readHmacBlocks(reader, hmacKey);
  let payload: Uint8Array;
  if (header.cipher === CipherId.Aes256) {
    if (header.iv.length !== 16) {
      throw new KdbxError('invalid-header', 'Invalid AES IV');
    }
    payload = await aesCbcDecrypt(encrypted, cipherKey, header.iv);
  } else if (header.cipher === CipherId.ChaCha20) {
    if (header.iv.length !== 12) {
      throw new KdbxError('invalid-header', 'Invalid ChaCha20 nonce');
    }
    payload = new ChaCha20Stream(cipherKey, header.iv).process(encrypted);
  } else {
    throw new KdbxError('unsupported-cipher', `Unsupported KDBX cipher: ${header.cipher}`);
  }

  if (header.compression === Compression.GZip) {
    try {
      payload = gunzipSync(payload);
    } catch (error) {
      throw new KdbxError('corrupt-data', 'Invalid compressed KDBX payload', { cause: error });
    }
  } else if (header.compression !== Compression.None) {
    throw new KdbxError('invalid-header', `Unsupported compression: ${header.compression}`);
  }

  const inner = readInnerHeader(payload);
  const protectedStream = await createProtectedStream(inner.streamId, inner.streamKey);
  const database = parseDatabaseXml(inner.xml, protectedStream, inner.binaries);
  database.header = {
    version: header.version,
    cipher: header.cipher,
    compression: header.compression,
    kdfParameters: header.kdfParameters,
    publicCustomData: header.publicCustomData,
  };
  return database;
}

export async function saveKdbx(
  database: KdbxDatabase,
  credentials: KdbxCredentials,
  options: SaveKdbxOptions = {},
): Promise<Uint8Array> {
  const cipher = options.cipher ?? database.header.cipher;
  const compression = options.compression ?? database.header.compression;
  if (cipher !== CipherId.Aes256 && cipher !== CipherId.ChaCha20) {
    throw new KdbxError('unsupported-cipher', `Unsupported KDBX cipher: ${String(cipher)}`);
  }
  if (compression !== Compression.None && compression !== Compression.GZip) {
    throw new KdbxError('invalid-header', `Unsupported compression: ${String(compression)}`);
  }

  const kdfParameters = new Map(database.header.kdfParameters);
  if (!(kdfParameters.get('S') instanceof Uint8Array)) {
    throw new KdbxError('invalid-header', 'KDF salt or seed is missing');
  }
  kdfParameters.set('S', randomBytes(32));
  database.header.kdfParameters = kdfParameters;
  database.header.cipher = cipher;
  database.header.compression = compression;
  database.header.version = KDBX_VERSION_4_1;

  const innerStreamKey = randomBytes(64);
  const protectedStream = await createProtectedStream(InnerStream.ChaCha20, innerStreamKey);
  const binaries: KdbxBinary[] = [];
  const xml = serializeDatabaseXml(database, protectedStream, binaries);
  let payload = writeInnerHeader(InnerStream.ChaCha20, innerStreamKey, binaries, xml);
  if (compression === Compression.GZip) {
    payload = gzipSync(payload);
  }

  const masterSeed = randomBytes(32);
  const iv = randomBytes(cipher === CipherId.Aes256 ? 16 : 12);
  const header = writeOuterHeader({
    version: KDBX_VERSION_4_1,
    cipher,
    compression,
    masterSeed,
    iv,
    kdfParameters,
    publicCustomData: database.header.publicCustomData,
  });
  const compositeKey = await credentials.getCompositeKey();
  const transformedKey = await deriveKdfKey(
    compositeKey,
    kdfParameters,
    resolveKdfLimits(options.kdfLimits),
  );
  const { cipherKey, hmacKey } = await deriveKeys(masterSeed, transformedKey);

  const encrypted =
    cipher === CipherId.Aes256
      ? await aesCbcEncrypt(payload, cipherKey, iv)
      : new ChaCha20Stream(cipherKey, iv).process(payload);
  const headerHash = await sha256(header);
  const headerHmacKey = await deriveBlockHmacKey(hmacKey, 0xffffffffffffffffn);
  const headerHmac = await hmacSha256(headerHmacKey, header);
  const blocks = await writeHmacBlocks(encrypted, hmacKey);
  return concatBytes(header, headerHash, headerHmac, blocks);
}

function readOuterHeader(reader: BinaryReader): OuterHeader {
  const start = reader.offset;
  if (reader.readUint32() !== KDBX_SIGNATURE_1 || reader.readUint32() !== KDBX_SIGNATURE_2) {
    throw new KdbxError('invalid-signature', 'Not a KDBX file');
  }
  const version = reader.readUint32();
  if (version >>> 16 !== 4 || version > KDBX_VERSION_4_1) {
    throw new KdbxError(
      'unsupported-version',
      `Unsupported KDBX version: 0x${version.toString(16)}`,
    );
  }

  const fields = new Map<number, Uint8Array>();
  while (true) {
    const id = reader.readUint8();
    const length = reader.readInt32();
    if (length < 0) {
      throw new KdbxError('invalid-header', 'Negative outer header field length');
    }
    const field = reader.readBytes(length);
    if (id === 0) {
      if (!equalBytes(field, Uint8Array.of(0x0d, 0x0a, 0x0d, 0x0a))) {
        throw new KdbxError('invalid-header', 'Invalid outer header terminator');
      }
      break;
    }
    if (fields.has(id)) {
      throw new KdbxError('invalid-header', `Duplicate outer header field: ${id}`);
    }
    fields.set(id, field);
  }

  const cipherBytes = requiredHeaderField(fields, 2, 16);
  const compressionBytes = requiredHeaderField(fields, 3, 4);
  const masterSeed = requiredHeaderField(fields, 4, 32);
  const iv = requiredHeaderField(fields, 7);
  const kdf = requiredHeaderField(fields, 11);
  const customData = fields.get(12);
  return {
    bytes: reader.bytes.slice(start, reader.offset),
    version,
    cipher: bytesToHex(cipherBytes),
    compression: new DataView(compressionBytes.buffer, compressionBytes.byteOffset, 4).getUint32(
      0,
      true,
    ),
    masterSeed,
    iv,
    kdfParameters: readVariantDictionary(kdf),
    publicCustomData: customData ? readVariantDictionary(customData) : new Map(),
  };
}

function writeOuterHeader(header: Omit<OuterHeader, 'bytes'>): Uint8Array {
  const writer = new BinaryWriter()
    .writeUint32(KDBX_SIGNATURE_1)
    .writeUint32(KDBX_SIGNATURE_2)
    .writeUint32(header.version);
  writeHeaderField(writer, 2, hexToBytes(header.cipher));
  writeHeaderField(writer, 3, new BinaryWriter().writeUint32(header.compression).toUint8Array());
  writeHeaderField(writer, 4, header.masterSeed);
  writeHeaderField(writer, 7, header.iv);
  writeHeaderField(writer, 11, writeVariantDictionary(header.kdfParameters));
  if (header.publicCustomData.size > 0) {
    writeHeaderField(writer, 12, writeVariantDictionary(header.publicCustomData));
  }
  writeHeaderField(writer, 0, Uint8Array.of(0x0d, 0x0a, 0x0d, 0x0a));
  return writer.toUint8Array();
}

function readInnerHeader(value: Uint8Array): InnerHeader {
  const reader = new BinaryReader(value);
  let streamId: number | undefined;
  let streamKey: Uint8Array | undefined;
  const binaries: KdbxBinary[] = [];
  while (true) {
    const id = reader.readUint8();
    const length = reader.readInt32();
    if (length < 0) {
      throw new KdbxError('invalid-header', 'Negative inner header field length');
    }
    const field = reader.readBytes(length);
    if (id === 0) {
      if (field.length !== 0) {
        throw new KdbxError('invalid-header', 'Invalid inner header terminator');
      }
      break;
    }
    if (id === 1) {
      if (streamId !== undefined || field.length !== 4) {
        throw new KdbxError('invalid-header', 'Invalid inner stream identifier');
      }
      streamId = new DataView(field.buffer, field.byteOffset, 4).getUint32(0, true);
    } else if (id === 2) {
      if (streamKey) {
        throw new KdbxError('invalid-header', 'Duplicate inner stream key');
      }
      streamKey = field;
    } else if (id === 3) {
      if (field.length < 1) {
        throw new KdbxError('invalid-header', 'Invalid inner binary');
      }
      binaries.push(new KdbxBinary(field.subarray(1), { protected: (field[0] & 1) !== 0 }));
    }
  }
  if (streamId === undefined || !streamKey) {
    throw new KdbxError('invalid-header', 'Inner stream parameters are missing');
  }
  return { streamId, streamKey, binaries, xml: decodeUtf8(reader.readBytes(reader.remaining)) };
}

function writeInnerHeader(
  streamId: number,
  streamKey: Uint8Array,
  binaries: KdbxBinary[],
  xml: string,
): Uint8Array {
  const writer = new BinaryWriter();
  writeHeaderField(writer, 1, new BinaryWriter().writeUint32(streamId).toUint8Array());
  writeHeaderField(writer, 2, streamKey);
  for (const binary of binaries) {
    writeHeaderField(writer, 3, concatBytes(Uint8Array.of(binary.protected ? 1 : 0), binary.data));
  }
  writeHeaderField(writer, 0, new Uint8Array());
  return concatBytes(writer.toUint8Array(), utf8(xml));
}

async function createProtectedStream(id: number, key: Uint8Array): Promise<ProtectedStream> {
  if (id === InnerStream.ChaCha20) {
    const hash = await sha512(key);
    return new ChaCha20Stream(hash.subarray(0, 32), hash.subarray(32, 44));
  }
  if (id === InnerStream.Salsa20) {
    return new Salsa20Stream(
      await sha256(key),
      Uint8Array.of(0xe8, 0x30, 0x09, 0x4b, 0x97, 0x20, 0x5d, 0x2a),
    );
  }
  if (id === InnerStream.None) {
    return { process: value => value.slice() };
  }
  throw new KdbxError('invalid-header', `Unsupported inner stream: ${id}`);
}

function requiredHeaderField(
  fields: Map<number, Uint8Array>,
  id: number,
  expectedLength?: number,
): Uint8Array {
  const value = fields.get(id);
  if (!value || (expectedLength !== undefined && value.length !== expectedLength)) {
    throw new KdbxError('invalid-header', `Missing or invalid outer header field: ${id}`);
  }
  return value;
}

function writeHeaderField(writer: BinaryWriter, id: number, value: Uint8Array): void {
  writer.writeUint8(id).writeInt32(value.length).writeBytes(value);
}

function normalizeCredentials(credentials: KdbxCredentials | string): KdbxCredentials {
  return typeof credentials === 'string'
    ? new KdbxCredentials({ password: credentials })
    : credentials;
}

function resolveKdfLimits(overrides: Partial<KdbxKdfLimits> | undefined): KdbxKdfLimits {
  return { ...defaultKdbxKdfLimits, ...overrides };
}
