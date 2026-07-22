import { xchacha20poly1305 } from '@noble/ciphers/chacha.js';
import { ed25519, x25519 } from '@noble/curves/ed25519.js';
import { hkdf } from '@noble/hashes/hkdf.js';
import { sha256 } from '@noble/hashes/sha2.js';

export type MessageFrame = {
  version: 1;
  timestamp: number;
  nonce: string;
  ephemeralPublicKey: string | null;
  publicKey: string;
  payload: string | null;
  signature: string;
};

export interface Relay {
  readonly id: string;
  connect(publicKeyBundle: string): Promise<void>;
  send(frame: MessageFrame): Promise<MessageFrame | null>;
}

export interface ClientStore {
  loadDeviceKey(): Promise<Uint8Array>;
  loadTrustedServer(relayId: string): Promise<string | undefined>;
  saveTrustedServer(relayId: string, bundle: string): Promise<void>;
}

const FRAME_TIMESTAMP_TOLERANCE_MS = 500;
const NONCE_CACHE_CAPACITY = 2048;
const FRAME_TRANSCRIPT_PREFIX = 'keeless-frame-v1';
const PAYLOAD_HEADER_PREFIX = 'keeless-payload-header-v1';
const PAYLOAD_HKDF_INFO = new TextEncoder().encode('keeless-payload-v1');
const DEVICE_SIGNING_INFO = new TextEncoder().encode('keeless-device-ed25519-v1');
const DEVICE_ENCRYPTION_INFO = new TextEncoder().encode('keeless-device-x25519-v1');
const EMPTY_SALT = new Uint8Array();
const encoder = new TextEncoder();

type Identity = {
  signingSecret: Uint8Array;
  encryptionSecret: Uint8Array;
  bundle: string;
};

type PublicKeyBundle = { signing: Uint8Array; encryption: Uint8Array };

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

const parseBundle = (value: string): PublicKeyBundle => {
  const parts = value.split('.');
  if (parts.length !== 3 || parts[0] !== 'v1') {
    throw new Error('Invalid public key bundle');
  }
  const signing = decodeBase64Url(parts[1]);
  const encryption = decodeBase64Url(parts[2]);
  if (signing.length !== 32 || encryption.length !== 32 || encryption.every(byte => byte === 0)) {
    throw new Error('Invalid public key bundle');
  }
  return { signing, encryption };
};

const transcript = (frame: MessageFrame) =>
  `${FRAME_TRANSCRIPT_PREFIX}|${frame.timestamp}|${frame.nonce}|${frame.ephemeralPublicKey ?? ''}|${frame.publicKey}|${frame.payload ?? ''}`;
const headerTranscript = (frame: MessageFrame) =>
  `${PAYLOAD_HEADER_PREFIX}|${frame.timestamp}|${frame.nonce}|${frame.ephemeralPublicKey ?? ''}|${frame.publicKey}`;
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

const deriveIdentity = async (store: ClientStore): Promise<Identity> => {
  const deviceKey = await store.loadDeviceKey();
  try {
    const signingSecret = hkdf(sha256, deviceKey, EMPTY_SALT, DEVICE_SIGNING_INFO, 32);
    const encryptionSecret = hkdf(sha256, deviceKey, EMPTY_SALT, DEVICE_ENCRYPTION_INFO, 32);
    return {
      signingSecret,
      encryptionSecret,
      bundle: `v1.${encodeBase64Url(ed25519.getPublicKey(signingSecret))}.${encodeBase64Url(x25519.getPublicKey(encryptionSecret))}`,
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

  static async connect(relay: Relay, store: ClientStore = new IndexedDbClientStore()) {
    const identity = await deriveIdentity(store);
    await relay.connect(identity.bundle);
    const request = signFrame(
      {
        version: 1,
        timestamp: Date.now(),
        nonce: encodeBase64Url(randomBytes(24)),
        ephemeralPublicKey: null,
        publicKey: identity.bundle,
        payload: null,
        signature: '',
      },
      identity.signingSecret,
    );
    const response = await relay.send(request);
    if (!response || response.payload !== null || response.ephemeralPublicKey !== null) {
      throw new Error('Server rejected the handshake');
    }
    const trusted = await store.loadTrustedServer(relay.id);
    verifyFrame(response, trusted);
    if (!trusted) {
      await store.saveTrustedServer(relay.id, response.publicKey);
    }
    return new Client(relay, identity, response.publicKey, parseBundle(response.publicKey));
  }

  request(payload: Uint8Array): Promise<Uint8Array> {
    const request = this.queue.then(() => this.performRequest(payload));
    this.queue = request.then(
      () => undefined,
      () => undefined,
    );
    return request;
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
      payload: null,
      signature: '',
    };
    try {
      frame.payload = encodeBase64Url(
        xchacha20poly1305(key, nonce, encoder.encode(headerTranscript(frame))).encrypt(payload),
      );
      frame = signFrame(frame, this.identity.signingSecret);
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
    verifyFrame(response, this.serverBundle);
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

  loadTrustedServer(relayId: string) {
    return this.read<string>(`trusted-server-v1:${relayId}`);
  }

  saveTrustedServer(relayId: string, bundle: string) {
    parseBundle(bundle);
    return this.write(`trusted-server-v1:${relayId}`, bundle);
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
