import { xchacha20poly1305 } from '@noble/ciphers/chacha.js';
import { ed25519, x25519 } from '@noble/curves/ed25519.js';
import { hkdf } from '@noble/hashes/hkdf.js';
import { sha256 } from '@noble/hashes/sha2.js';
import { download as downloadTransfer, upload as uploadTransfer } from './transfer';

export {
  MAX_TRANSFER_CHUNK_SIZE,
  MAX_TRANSFER_SIZE,
  TRANSFER_MAGIC,
  download,
  upload,
} from './transfer';

export type MessageFrame = {
  version: 1;
  timestamp: number;
  nonce: string;
  ephemeralPublicKey: string | null;
  publicKey: string;
  recipient: string;
  payload: string | null;
  signature: string;
};

export type KeyScope = 'core_untrusted' | 'core' | 'app' | 'passkey';

export interface Relay {
  readonly id: string;
  connect(clientBundle: string): Promise<string>;
  send(frame: MessageFrame): Promise<MessageFrame | null>;
}

export interface ClientStore {
  loadDeviceKey(): Promise<Uint8Array>;
  loadTrustedServer(endpointId: string): Promise<string | undefined>;
  saveTrustedServer(endpointId: string, bundle: string): Promise<void>;
}

export const MAX_FRAME_SIZE = 1024 * 1024;
const FRAME_TIMESTAMP_TOLERANCE_MS = 500;
const NONCE_CACHE_CAPACITY = 2048;
const FRAME_TRANSCRIPT_PREFIX = 'keeless-frame-v1';
const PAYLOAD_HEADER_PREFIX = 'keeless-payload-header-v1';
const PAYLOAD_HKDF_INFO = new TextEncoder().encode('keeless-payload-v1');
const DEVICE_SIGNING_INFO = new TextEncoder().encode('keeless-device-ed25519-v1');
const DEVICE_ENCRYPTION_INFO = new TextEncoder().encode('keeless-device-x25519-v1');
const EMPTY_SALT = new Uint8Array();
const encoder = new TextEncoder();

const frameSize = (frame: MessageFrame) => encoder.encode(JSON.stringify(frame)).byteLength;

type Identity = {
  signingSecret: Uint8Array;
  encryptionSecret: Uint8Array;
  bundle: string;
  scope: KeyScope;
};

type PublicKeyBundle = { signing: Uint8Array; encryption: Uint8Array; scope: KeyScope };

const randomBytes = (length: number) => crypto.getRandomValues(new Uint8Array(length));

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

export const parseBundle = (value: string): PublicKeyBundle => {
  const parts = value.split('.');
  if (parts.length !== 4 || parts[0] !== 'v1') {
    throw new Error('Invalid public key bundle');
  }
  const signing = decodeBase64Url(parts[1]);
  const encryption = decodeBase64Url(parts[2]);
  if (signing.length !== 32 || encryption.length !== 32 || encryption.every(byte => byte === 0)) {
    throw new Error('Invalid public key bundle');
  }
  const scope = parts[3];
  if (!['core_untrusted', 'core', 'app', 'passkey'].includes(scope)) {
    throw new Error('Invalid public key bundle');
  }
  return { signing, encryption, scope: scope as KeyScope };
};

const transcript = (frame: MessageFrame) =>
  `${FRAME_TRANSCRIPT_PREFIX}|${frame.timestamp}|${frame.nonce}|${frame.ephemeralPublicKey ?? ''}|${frame.publicKey}|${frame.recipient}|${frame.payload ?? ''}`;
const headerTranscript = (frame: MessageFrame) =>
  `${PAYLOAD_HEADER_PREFIX}|${frame.timestamp}|${frame.nonce}|${frame.ephemeralPublicKey ?? ''}|${frame.publicKey}|${frame.recipient}`;
const signFrame = (frame: MessageFrame, secret: Uint8Array): MessageFrame => ({
  ...frame,
  signature: encodeBase64Url(ed25519.sign(encoder.encode(transcript(frame)), secret)),
});

const verifyFrame = (frame: MessageFrame, expectedBundle?: string) => {
  if (
    frame.version !== 1 ||
    !Number.isSafeInteger(frame.timestamp) ||
    typeof frame.nonce !== 'string' ||
    typeof frame.publicKey !== 'string' ||
    typeof frame.recipient !== 'string' ||
    typeof frame.signature !== 'string' ||
    (frame.ephemeralPublicKey !== null && typeof frame.ephemeralPublicKey !== 'string') ||
    (frame.payload !== null && typeof frame.payload !== 'string') ||
    decodeBase64Url(frame.nonce).length !== 24 ||
    Math.abs(Date.now() - frame.timestamp) > FRAME_TIMESTAMP_TOLERANCE_MS
  ) {
    throw new Error('Relay returned an invalid or stale message frame');
  }
  if (expectedBundle && frame.publicKey !== expectedBundle) {
    throw new Error('Server identity changed');
  }
  const bundle = parseBundle(frame.publicKey);
  const signature = decodeBase64Url(frame.signature);
  if (
    signature.length !== 64 ||
    !ed25519.verify(signature, encoder.encode(transcript(frame)), bundle.signing, { zip215: false })
  ) {
    throw new Error('Relay returned an invalid signature');
  }
  return bundle;
};

const deriveIdentity = async (store: ClientStore, scope: KeyScope): Promise<Identity> => {
  const deviceKey = await store.loadDeviceKey();
  try {
    const signingSecret = hkdf(sha256, deviceKey, EMPTY_SALT, DEVICE_SIGNING_INFO, 32);
    const encryptionSecret = hkdf(sha256, deviceKey, EMPTY_SALT, DEVICE_ENCRYPTION_INFO, 32);
    return {
      signingSecret,
      encryptionSecret,
      bundle: `v1.${encodeBase64Url(ed25519.getPublicKey(signingSecret))}.${encodeBase64Url(x25519.getPublicKey(encryptionSecret))}.${scope}`,
      scope,
    };
  } finally {
    deviceKey.fill(0);
  }
};

export class Client {
  private queue: Promise<void> = Promise.resolve();
  private readonly responseNonces = new Map<string, number>();

  private constructor(
    private readonly relay: Relay,
    private readonly identity: Identity,
    private readonly serverBundle: string,
    private readonly serverKeys: PublicKeyBundle,
  ) {}

  static async connect(
    relay: Relay,
    scope: KeyScope,
    recipient?: string,
    store: ClientStore = new IndexedDbClientStore(),
    endpointId = relay.id,
  ) {
    const identity = await deriveIdentity(store, scope);
    const serverBundle = recipient ?? (await relay.connect(identity.bundle));
    parseBundle(serverBundle);
    const trusted = await store.loadTrustedServer(endpointId);
    if (trusted && trusted !== serverBundle) {
      throw new Error('Server identity changed');
    }
    const request = signFrame(
      {
        version: 1,
        timestamp: Date.now(),
        nonce: encodeBase64Url(randomBytes(24)),
        ephemeralPublicKey: null,
        publicKey: identity.bundle,
        recipient: serverBundle,
        payload: null,
        signature: '',
      },
      identity.signingSecret,
    );
    const response = await relay.send(request);
    if (!response || response.payload !== null || response.ephemeralPublicKey !== null) {
      throw new Error('Server rejected the handshake');
    }
    if (response?.recipient !== identity.bundle) {
      throw new Error('Server returned a frame for another recipient');
    }
    verifyFrame(response, serverBundle);
    if (!trusted) {
      await store.saveTrustedServer(endpointId, response.publicKey);
    }
    return new Client(relay, identity, serverBundle, parseBundle(serverBundle));
  }

  request(payload: Uint8Array): Promise<Uint8Array> {
    const request = this.queue.then(() => this.performRequest(payload));
    this.queue = request.then(
      () => undefined,
      () => undefined,
    );
    return request;
  }

  upload(file: Blob) {
    return uploadTransfer(this, file);
  }

  download(transferId: string) {
    return downloadTransfer(this, transferId);
  }

  private async performRequest(payload: Uint8Array) {
    const nonce = randomBytes(24);
    const ephemeral = x25519.keygen();
    const shared = x25519.getSharedSecret(ephemeral.secretKey, this.serverKeys.encryption);
    const key = hkdf(sha256, shared, nonce, PAYLOAD_HKDF_INFO, 32);
    let frame: MessageFrame = {
      version: 1,
      timestamp: Date.now(),
      nonce: encodeBase64Url(nonce),
      ephemeralPublicKey: encodeBase64Url(ephemeral.publicKey),
      publicKey: this.identity.bundle,
      recipient: this.serverBundle,
      payload: null,
      signature: '',
    };
    try {
      frame.payload = encodeBase64Url(
        xchacha20poly1305(key, nonce, encoder.encode(headerTranscript(frame))).encrypt(payload),
      );
      frame = signFrame(frame, this.identity.signingSecret);
      if (frameSize(frame) > MAX_FRAME_SIZE) {
        throw new Error('Encrypted frame exceeds the 1 MiB limit');
      }
    } finally {
      nonce.fill(0);
      ephemeral.secretKey.fill(0);
      shared.fill(0);
      key.fill(0);
    }
    const response = await this.relay.send(frame);
    if (!response?.payload || !response.ephemeralPublicKey) {
      throw new Error('Server rejected the request');
    }
    if (frameSize(response) > MAX_FRAME_SIZE) {
      throw new Error('Relay returned an oversized message frame');
    }
    verifyFrame(response, this.serverBundle);
    if (response.recipient !== this.identity.bundle) {
      throw new Error('Server returned a frame for another recipient');
    }
    const acceptedAt = performance.now();
    for (const [cachedNonce, observedAt] of this.responseNonces) {
      if (acceptedAt - observedAt > FRAME_TIMESTAMP_TOLERANCE_MS) {
        this.responseNonces.delete(cachedNonce);
      }
    }
    if (
      this.responseNonces.has(response.nonce) ||
      this.responseNonces.size >= NONCE_CACHE_CAPACITY
    ) {
      throw new Error('Server returned a replayed message frame');
    }
    const responseNonce = decodeBase64Url(response.nonce);
    const responseEphemeral = decodeBase64Url(response.ephemeralPublicKey);
    const responseShared = x25519.getSharedSecret(
      this.identity.encryptionSecret,
      responseEphemeral,
    );
    const responseKey = hkdf(sha256, responseShared, responseNonce, PAYLOAD_HKDF_INFO, 32);
    try {
      const plaintext = xchacha20poly1305(
        responseKey,
        responseNonce,
        encoder.encode(headerTranscript(response)),
      ).decrypt(decodeBase64Url(response.payload));
      this.responseNonces.set(response.nonce, acceptedAt);
      return plaintext;
    } finally {
      responseNonce.fill(0);
      responseEphemeral.fill(0);
      responseShared.fill(0);
      responseKey.fill(0);
    }
  }
}

const DATABASE_NAME = 'keeless-lesswire';
const STORE_NAME = 'client';
const DEVICE_KEY = 'device-key-v1';

export class IndexedDbClientStore implements ClientStore {
  private database?: Promise<IDBDatabase>;

  private open() {
    this.database ??= new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open(DATABASE_NAME, 1);
      request.onupgradeneeded = () => request.result.createObjectStore(STORE_NAME);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error ?? new Error('Failed to open wire storage'));
      request.onblocked = () => reject(new Error('Wire storage upgrade was blocked'));
    });
    return this.database;
  }

  async loadDeviceKey() {
    const existing = await this.read<ArrayBuffer>(DEVICE_KEY);
    if (existing?.byteLength === 32) {
      return new Uint8Array(existing);
    }
    const key = randomBytes(32);
    await this.write(DEVICE_KEY, key.buffer.slice(0));
    return key;
  }

  loadTrustedServer(endpointId: string) {
    return this.read<string>(`trusted-server-v1:${endpointId}`);
  }

  saveTrustedServer(endpointId: string, bundle: string) {
    parseBundle(bundle);
    return this.write(`trusted-server-v1:${endpointId}`, bundle);
  }

  private async read<T>(key: string) {
    const database = await this.open();
    return new Promise<T | undefined>((resolve, reject) => {
      const request = database.transaction(STORE_NAME).objectStore(STORE_NAME).get(key);
      request.onsuccess = () => resolve(request.result as T | undefined);
      request.onerror = () => reject(request.error ?? new Error('Failed to read wire storage'));
    });
  }

  private async write(key: string, value: unknown) {
    const database = await this.open();
    return new Promise<void>((resolve, reject) => {
      const transaction = database.transaction(STORE_NAME, 'readwrite');
      transaction.objectStore(STORE_NAME).put(value, key);
      transaction.oncomplete = () => resolve();
      transaction.onerror = () =>
        reject(transaction.error ?? new Error('Failed to write wire storage'));
      transaction.onabort = () =>
        reject(transaction.error ?? new Error('Wire storage write was aborted'));
    });
  }
}
