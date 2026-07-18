import { xchacha20poly1305 } from '@noble/ciphers/chacha.js';
import { ed25519, x25519 } from '@noble/curves/ed25519.js';
import { hkdf } from '@noble/hashes/hkdf.js';
import { sha256 } from '@noble/hashes/sha2.js';
import { loadDeviceKey, loadTrustedCore, saveTrustedCore } from './deviceIdentity';
import type { Host } from '@/types/Host';
import type { MessageFrame, Operation, OperationResponse, OperationSuccess } from '@keeless/schema';

const FRAME_VERSION = 1;
const FRAME_TIMESTAMP_TOLERANCE_MS = 500;
const FRAME_TRANSCRIPT_PREFIX = 'keeless-frame-v1';
const PAYLOAD_HEADER_PREFIX = 'keeless-payload-header-v1';
const PAYLOAD_HKDF_INFO = new TextEncoder().encode('keeless-payload-v1');
const DEVICE_SIGNING_INFO = new TextEncoder().encode('keeless-device-ed25519-v1');
const DEVICE_ENCRYPTION_INFO = new TextEncoder().encode('keeless-device-x25519-v1');
const EMPTY_SALT = new Uint8Array();
const encoder = new TextEncoder();
const decoder = new TextDecoder(undefined, { fatal: true });

export type OperationName = Operation['op'];
export type OperationArgs<TName extends OperationName> = Extract<Operation, { op: TName }>['args'];
export type OperationResult<TName extends OperationName> = Extract<
  OperationSuccess,
  { op: TName }
>['result'];

type ClientIdentity = {
  signingSecret: Uint8Array;
  signingPublic: Uint8Array;
  encryptionSecret: Uint8Array;
  encryptionPublic: Uint8Array;
  bundle: string;
};

type PublicKeyBundle = {
  signing: Uint8Array;
  encryption: Uint8Array;
};

export class CoreRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = 'CoreRequestError';
    this.code = code;
  }
}

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
  const binary = atob(padded);
  const bytes = Uint8Array.from(binary, character => character.charCodeAt(0));
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

const signFrame = (frame: MessageFrame, signingSecret: Uint8Array): MessageFrame => ({
  ...frame,
  signature: encodeBase64Url(ed25519.sign(encoder.encode(transcript(frame)), signingSecret)),
});

const validateFrameShape = (frame: MessageFrame) => {
  if (
    frame.version !== FRAME_VERSION ||
    !Number.isSafeInteger(frame.timestamp) ||
    typeof frame.nonce !== 'string' ||
    typeof frame.publicKey !== 'string' ||
    typeof frame.signature !== 'string' ||
    (frame.ephemeralPublicKey !== null && typeof frame.ephemeralPublicKey !== 'string') ||
    (frame.payload !== null && typeof frame.payload !== 'string')
  ) {
    throw new Error('Host returned an invalid message frame');
  }
  if (decodeBase64Url(frame.nonce).length !== 24) {
    throw new Error('Host returned an invalid nonce');
  }
  if (Math.abs(Date.now() - frame.timestamp) > FRAME_TIMESTAMP_TOLERANCE_MS) {
    throw new Error('Host returned a stale message frame');
  }
};

const verifyFrame = (frame: MessageFrame, expectedBundle?: string) => {
  validateFrameShape(frame);
  if (expectedBundle && frame.publicKey !== expectedBundle) {
    throw new Error('Host identity changed');
  }
  const bundle = parseBundle(frame.publicKey);
  const signature = decodeBase64Url(frame.signature);
  if (
    signature.length !== 64 ||
    !ed25519.verify(signature, encoder.encode(transcript(frame)), bundle.signing, { zip215: false })
  ) {
    throw new Error('Host returned an invalid signature');
  }
  return bundle;
};

const deriveIdentity = async (): Promise<ClientIdentity> => {
  const deviceKey = await loadDeviceKey();
  try {
    const signingSecret = hkdf(sha256, deviceKey, EMPTY_SALT, DEVICE_SIGNING_INFO, 32);
    const encryptionSecret = hkdf(sha256, deviceKey, EMPTY_SALT, DEVICE_ENCRYPTION_INFO, 32);
    const signingPublic = ed25519.getPublicKey(signingSecret);
    const encryptionPublic = x25519.getPublicKey(encryptionSecret);
    return {
      signingSecret,
      signingPublic,
      encryptionSecret,
      encryptionPublic,
      bundle: `v1.${encodeBase64Url(signingPublic)}.${encodeBase64Url(encryptionPublic)}`,
    };
  } finally {
    deviceKey.fill(0);
  }
};

const handshake = async (host: Host, identity: ClientIdentity) => {
  const request = signFrame(
    {
      version: FRAME_VERSION,
      timestamp: Date.now(),
      nonce: encodeBase64Url(randomBytes(24)),
      ephemeralPublicKey: null,
      publicKey: identity.bundle,
      payload: null,
      signature: '',
    },
    identity.signingSecret,
  );
  const response = await host.send(request);
  if (!response || response.payload !== null || response.ephemeralPublicKey !== null) {
    throw new Error('Host rejected the handshake');
  }

  const trusted = await loadTrustedCore(host.id);
  verifyFrame(response, trusted);
  if (!trusted) {
    await saveTrustedCore(host.id, response.publicKey);
  }
  return response.publicKey;
};

export class RequestClient {
  readonly host: Host;
  private readonly identity: ClientIdentity;
  private readonly coreBundle: string;
  private readonly corePublicKeys: PublicKeyBundle;
  private requestQueue: Promise<void> = Promise.resolve();

  private constructor(host: Host, identity: ClientIdentity, coreBundle: string) {
    this.host = host;
    this.identity = identity;
    this.coreBundle = coreBundle;
    this.corePublicKeys = parseBundle(coreBundle);
  }

  static async connect(host: Host) {
    const identity = await deriveIdentity();
    await host.connect(identity.bundle);
    const coreBundle = await handshake(host, identity);
    return new RequestClient(host, identity, coreBundle);
  }

  async request<TName extends OperationName>(
    op: TName,
    args: OperationArgs<TName>,
  ): Promise<OperationResult<TName>> {
    const request = this.requestQueue.then(() => this.performRequest(op, args));
    this.requestQueue = request.then(
      () => undefined,
      () => undefined,
    );
    return request;
  }

  private async performRequest<TName extends OperationName>(
    op: TName,
    args: OperationArgs<TName>,
  ): Promise<OperationResult<TName>> {
    const requestId = crypto.randomUUID();
    const plaintext = encoder.encode(JSON.stringify({ requestId, op, args }));
    const nonce = randomBytes(24);
    const ephemeral = x25519.keygen();
    const shared = x25519.getSharedSecret(ephemeral.secretKey, this.corePublicKeys.encryption);
    const key = hkdf(sha256, shared, nonce, PAYLOAD_HKDF_INFO, 32);
    let frame: MessageFrame = {
      version: FRAME_VERSION,
      timestamp: Date.now(),
      nonce: encodeBase64Url(nonce),
      ephemeralPublicKey: encodeBase64Url(ephemeral.publicKey),
      publicKey: this.identity.bundle,
      payload: null,
      signature: '',
    };
    try {
      const cipher = xchacha20poly1305(key, nonce, encoder.encode(headerTranscript(frame)));
      frame = {
        ...frame,
        payload: encodeBase64Url(cipher.encrypt(plaintext)),
      };
      frame = signFrame(frame, this.identity.signingSecret);
    } finally {
      plaintext.fill(0);
      nonce.fill(0);
      ephemeral.secretKey.fill(0);
      shared.fill(0);
      key.fill(0);
    }

    const responseFrame = await this.host.send(frame);
    if (!responseFrame || !responseFrame.payload || !responseFrame.ephemeralPublicKey) {
      throw new Error('Host rejected the request');
    }
    verifyFrame(responseFrame, this.coreBundle);

    const responseNonce = decodeBase64Url(responseFrame.nonce);
    const responseEphemeral = decodeBase64Url(responseFrame.ephemeralPublicKey);
    const responseShared = x25519.getSharedSecret(
      this.identity.encryptionSecret,
      responseEphemeral,
    );
    const responseKey = hkdf(sha256, responseShared, responseNonce, PAYLOAD_HKDF_INFO, 32);
    let responseBytes: Uint8Array;
    try {
      responseBytes = xchacha20poly1305(
        responseKey,
        responseNonce,
        encoder.encode(headerTranscript(responseFrame)),
      ).decrypt(decodeBase64Url(responseFrame.payload));
    } finally {
      responseNonce.fill(0);
      responseEphemeral.fill(0);
      responseShared.fill(0);
      responseKey.fill(0);
    }

    try {
      const response = JSON.parse(decoder.decode(responseBytes)) as OperationResponse;
      if (response.requestId !== requestId) {
        throw new Error('Host returned a mismatched request ID');
      }
      if (response.status === 'error') {
        throw new CoreRequestError(response.error.code, response.error.message);
      }
      if (response.op !== op) {
        throw new Error('Host returned a mismatched operation');
      }
      return response.result as OperationResult<TName>;
    } finally {
      responseBytes.fill(0);
    }
  }
}

export const getRequestClient = (host: Host) => RequestClient.connect(host);
