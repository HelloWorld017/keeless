export const MAX_TRANSFER_CHUNK_SIZE = 760 * 1024;
export const MAX_TRANSFER_SIZE = 192 * 1024 * 1024;
export const TRANSFER_MAGIC = 0x21;

const TRANSFER_VERSION = 1;
const TRANSFER_ID_SIZE = 16;
const TRANSFER_HEADER_SIZE = 3;
const transferKinds = {
  BeginUpload: 1,
  BeginUploadResponse: 2,
  UploadChunk: 3,
  UploadChunkResponse: 4,
  BeginDownload: 5,
  BeginDownloadResponse: 6,
  DownloadChunk: 7,
  DownloadChunkResponse: 8,
  Abort: 9,
  Finish: 10,
} as const;

export type TransferTransport = {
  request(payload: Uint8Array): Promise<Uint8Array>;
};

const encodeBase64Url = (value: Uint8Array) => {
  let binary = '';
  for (const byte of value) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary).replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/u, '');
};

const decodeBase64Url = (value: string) => {
  if (!/^[A-Za-z0-9_-]*$/u.test(value)) {
    throw new Error('Invalid base64url value');
  }
  const padded = value
    .replaceAll('-', '+')
    .replaceAll('_', '/')
    .padEnd(Math.ceil(value.length / 4) * 4, '=');
  const bytes = Uint8Array.from(atob(padded), character => character.charCodeAt(0));
  if (encodeBase64Url(bytes) !== value) {
    throw new Error('Non-canonical base64url value');
  }
  return bytes;
};

const decodeTransferId = (value: string) => {
  const id = decodeBase64Url(value);
  if (id.length !== TRANSFER_ID_SIZE) {
    throw new Error('Invalid transfer ID');
  }
  return id;
};

const writeU64 = (view: DataView, offset: number, value: number) => {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error('Invalid transfer size or offset');
  }
  view.setUint32(offset, value >>> 0, true);
  view.setUint32(offset + 4, Math.floor(value / 2 ** 32), true);
};

const readU64 = (view: DataView, offset: number) => {
  const value = view.getUint32(offset, true) + view.getUint32(offset + 4, true) * 2 ** 32;
  if (!Number.isSafeInteger(value)) {
    throw new Error('Invalid transfer size or offset');
  }
  return value;
};

const transferPacket = (kind: number, size: number) => {
  const bytes = new Uint8Array(size);
  bytes[0] = TRANSFER_MAGIC;
  bytes[1] = TRANSFER_VERSION;
  bytes[2] = kind;
  return bytes;
};

const expectTransferPacket = (payload: Uint8Array, kind: number, minimumLength: number) => {
  if (
    payload.length < minimumLength ||
    payload[0] !== TRANSFER_MAGIC ||
    payload[1] !== TRANSFER_VERSION ||
    payload[2] !== kind
  ) {
    throw new Error('Server returned an invalid transfer response');
  }
};

const transferIdPacket = (kind: number, transferId: string, offset?: number) => {
  const id = decodeTransferId(transferId);
  const packet = transferPacket(
    kind,
    TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE + (offset === undefined ? 0 : 8),
  );
  packet.set(id, TRANSFER_HEADER_SIZE);
  if (offset !== undefined) {
    writeU64(new DataView(packet.buffer), TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE, offset);
  }
  id.fill(0);
  return packet;
};

const beginUpload = async (transport: TransferTransport, size: number) => {
  if (!Number.isSafeInteger(size) || size <= 0 || size > MAX_TRANSFER_SIZE) {
    throw new Error('Invalid upload size');
  }
  const packet = transferPacket(transferKinds.BeginUpload, TRANSFER_HEADER_SIZE + 8);
  writeU64(new DataView(packet.buffer), TRANSFER_HEADER_SIZE, size);
  const response = await transport.request(packet);
  try {
    expectTransferPacket(
      response,
      transferKinds.BeginUploadResponse,
      TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE,
    );
    if (response.length !== TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE) {
      throw new Error('Server returned an invalid transfer response');
    }
    return encodeBase64Url(response.slice(TRANSFER_HEADER_SIZE));
  } finally {
    packet.fill(0);
    response.fill(0);
  }
};

const uploadChunk = async (
  transport: TransferTransport,
  transferId: string,
  offset: number,
  chunk: Uint8Array,
) => {
  if (chunk.length > MAX_TRANSFER_CHUNK_SIZE) {
    throw new Error('Upload chunk exceeds the transfer limit');
  }
  const id = decodeTransferId(transferId);
  const packet = transferPacket(
    transferKinds.UploadChunk,
    TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE + 8 + chunk.length,
  );
  packet.set(id, TRANSFER_HEADER_SIZE);
  writeU64(new DataView(packet.buffer), TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE, offset);
  packet.set(chunk, TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE + 8);
  const response = await transport.request(packet);
  try {
    expectTransferPacket(
      response,
      transferKinds.UploadChunkResponse,
      TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE + 8,
    );
    if (response.length !== TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE + 8) {
      throw new Error('Server returned an invalid transfer response');
    }
    const responseId = response.slice(
      TRANSFER_HEADER_SIZE,
      TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE,
    );
    if (encodeBase64Url(responseId) !== transferId) {
      throw new Error('Server returned a mismatched transfer ID');
    }
    return readU64(new DataView(response.buffer, response.byteOffset, response.byteLength), 19);
  } finally {
    id.fill(0);
    packet.fill(0);
    response.fill(0);
  }
};

const finish = async (transport: TransferTransport, transferId: string) => {
  const packet = transferIdPacket(transferKinds.Finish, transferId);
  const response = await transport.request(packet);
  try {
    expectTransferIdResponse(response, transferKinds.Finish, transferId);
  } finally {
    packet.fill(0);
    response.fill(0);
  }
};

const abort = async (transport: TransferTransport, transferId: string) => {
  const packet = transferIdPacket(transferKinds.Abort, transferId);
  const response = await transport.request(packet);
  try {
    expectTransferIdResponse(response, transferKinds.Abort, transferId);
  } finally {
    packet.fill(0);
    response.fill(0);
  }
};

const beginDownload = async (transport: TransferTransport, transferId: string) => {
  const packet = transferIdPacket(transferKinds.BeginDownload, transferId);
  const response = await transport.request(packet);
  try {
    expectTransferPacket(
      response,
      transferKinds.BeginDownloadResponse,
      TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE + 8,
    );
    if (response.length !== TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE + 8) {
      throw new Error('Server returned an invalid transfer response');
    }
    const responseId = response.slice(
      TRANSFER_HEADER_SIZE,
      TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE,
    );
    if (encodeBase64Url(responseId) !== transferId) {
      throw new Error('Server returned a mismatched transfer ID');
    }
    const size = readU64(
      new DataView(response.buffer, response.byteOffset, response.byteLength),
      19,
    );
    if (size <= 0 || size > MAX_TRANSFER_SIZE) {
      throw new Error('Server returned an invalid download size');
    }
    return size;
  } finally {
    packet.fill(0);
    response.fill(0);
  }
};

const downloadChunk = async (transport: TransferTransport, transferId: string, offset: number) => {
  const packet = transferIdPacket(transferKinds.DownloadChunk, transferId, offset);
  const response = await transport.request(packet);
  try {
    expectTransferPacket(
      response,
      transferKinds.DownloadChunkResponse,
      TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE + 8 + 1,
    );
    const responseId = response.slice(
      TRANSFER_HEADER_SIZE,
      TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE,
    );
    if (encodeBase64Url(responseId) !== transferId) {
      throw new Error('Server returned a mismatched transfer ID');
    }
    const view = new DataView(response.buffer, response.byteOffset, response.byteLength);
    const responseOffset = readU64(view, 19);
    const done = response[27] === 1;
    if (response[27] > 1) {
      throw new Error('Server returned an invalid transfer response');
    }
    return { offset: responseOffset, done, bytes: response.slice(28) };
  } finally {
    packet.fill(0);
    response.fill(0);
  }
};

const expectTransferIdResponse = (response: Uint8Array, kind: number, transferId: string) => {
  expectTransferPacket(response, kind, TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE);
  if (response.length !== TRANSFER_HEADER_SIZE + TRANSFER_ID_SIZE) {
    throw new Error('Server returned an invalid transfer response');
  }
  if (encodeBase64Url(response.slice(TRANSFER_HEADER_SIZE)) !== transferId) {
    throw new Error('Server returned a mismatched transfer ID');
  }
};

export const upload = async (transport: TransferTransport, file: Blob) => {
  const transferId = await beginUpload(transport, file.size);
  let offset = 0;
  try {
    while (offset < file.size) {
      const chunk = new Uint8Array(
        await file.slice(offset, offset + MAX_TRANSFER_CHUNK_SIZE).arrayBuffer(),
      );
      try {
        const nextOffset = await uploadChunk(transport, transferId, offset, chunk);
        if (nextOffset !== offset + chunk.length) {
          throw new Error('Server returned an invalid upload offset');
        }
        offset = nextOffset;
      } finally {
        chunk.fill(0);
      }
    }
    await finish(transport, transferId);
    return transferId;
  } catch (error) {
    await abort(transport, transferId).catch(() => undefined);
    throw error;
  }
};

export const download = async (transport: TransferTransport, transferId: string) => {
  const size = await beginDownload(transport, transferId);
  const bytes = new Uint8Array(size);
  let offset = 0;
  try {
    while (offset < size) {
      const chunk = await downloadChunk(transport, transferId, offset);
      try {
        if (
          chunk.offset !== offset ||
          chunk.bytes.length === 0 ||
          chunk.bytes.length > size - offset
        ) {
          throw new Error('Server returned an invalid download chunk');
        }
        bytes.set(chunk.bytes, offset);
        offset += chunk.bytes.length;
        if (chunk.done !== (offset === size)) {
          throw new Error('Server returned an incomplete download');
        }
      } finally {
        chunk.bytes.fill(0);
      }
    }
    await finish(transport, transferId);
    return bytes;
  } catch (error) {
    bytes.fill(0);
    await abort(transport, transferId).catch(() => undefined);
    throw error;
  }
};
