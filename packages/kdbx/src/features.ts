import { asArrayBuffer, base64ToBytes } from './binary.ts';
import { PasskeyField, StandardField } from './constants.ts';
import type { KdbxEntry } from './model.ts';

export interface EntryUrl {
  field: string;
  value: string;
  primary: boolean;
}

export function getEntryUrls(entry: KdbxEntry): EntryUrl[] {
  const urls: EntryUrl[] = [];
  const primary = entry.get(StandardField.URL);
  if (primary) {
    urls.push({ field: StandardField.URL, value: primary, primary: true });
  }
  const additional = Array.from(entry.fields)
    .filter(([key, field]) => /^KP2A_URL(?:_\d+)?$/.test(key) && field.value)
    .sort(([left], [right]) => urlFieldIndex(left) - urlFieldIndex(right));
  for (const [field, value] of additional) {
    urls.push({ field, value: value.value, primary: false });
  }
  return urls;
}

export type TotpAlgorithm = 'SHA1' | 'SHA256' | 'SHA512';

export interface TotpConfig {
  source: 'keepass2' | 'keeotp' | 'otpauth' | 'keetraytotp';
  secret: Uint8Array;
  period: number;
  digits: number;
  algorithm: TotpAlgorithm;
  encoder: 'rfc6238' | 'steam';
  issuer?: string;
  account?: string;
  timeCorrectionUrl?: string;
}

export function parseTotp(entry: KdbxEntry): TotpConfig | undefined {
  return parseKeeTrayTotp(entry) ?? parseOtpField(entry) ?? parseKeePass2Totp(entry);
}

export async function generateTotp(config: TotpConfig, timestamp = Date.now()): Promise<string> {
  if (!Number.isSafeInteger(config.period) || config.period <= 0) {
    throw new TypeError('TOTP period must be a positive integer');
  }
  const counter = BigInt(Math.floor(timestamp / 1000 / config.period));
  const message = new Uint8Array(8);
  new DataView(message.buffer).setBigUint64(0, counter, false);
  const key = await crypto.subtle.importKey(
    'raw',
    asArrayBuffer(config.secret),
    { name: 'HMAC', hash: webCryptoHash(config.algorithm) },
    false,
    ['sign'],
  );
  const digest = new Uint8Array(await crypto.subtle.sign('HMAC', key, message));
  const offset = digest[digest.length - 1] & 0x0f;
  const code =
    (((digest[offset] & 0x7f) << 24) |
      (digest[offset + 1] << 16) |
      (digest[offset + 2] << 8) |
      digest[offset + 3]) >>>
    0;
  if (config.encoder === 'steam') {
    const alphabet = '23456789BCDFGHJKMNPQRTVWXY';
    let value = code;
    let result = '';
    for (let index = 0; index < 5; index += 1) {
      result += alphabet[value % alphabet.length];
      value = Math.floor(value / alphabet.length);
    }
    return result;
  }
  const modulus = 10 ** config.digits;
  return String(code % modulus).padStart(config.digits, '0');
}

export interface KeePassXCPasskey {
  relyingParty: string;
  username: string;
  credentialId: string;
  userHandle: string;
  privateKeyPem: string;
  backupEligible: boolean;
  backupState: boolean;
}

export function hasPasskey(entry: KdbxEntry): boolean {
  return Array.from(entry.fields.keys()).some(key => key.startsWith('KPEX_PASSKEY'));
}

export function parsePasskey(entry: KdbxEntry): KeePassXCPasskey | undefined {
  if (!hasPasskey(entry)) {
    return undefined;
  }
  const credentialId = entry.fields.has(PasskeyField.GeneratedUserId)
    ? entry.get(PasskeyField.GeneratedUserId)
    : entry.get(PasskeyField.CredentialId);
  const username = entry.fields.has(PasskeyField.CompatibleUsername)
    ? entry.get(PasskeyField.CompatibleUsername)
    : entry.get(PasskeyField.Username);
  const relyingParty = entry.get(PasskeyField.RelyingParty);
  const userHandle = entry.get(PasskeyField.UserHandle);
  const privateKeyPem = entry.get(PasskeyField.PrivateKeyPem);
  if (!credentialId || !relyingParty || !userHandle || !isPrivateKeyPem(privateKeyPem)) {
    return undefined;
  }
  if (!isBase64Url(credentialId) || !isBase64Url(userHandle)) {
    return undefined;
  }
  return {
    relyingParty,
    username,
    credentialId: normalizeBase64Url(credentialId),
    userHandle: normalizeBase64Url(userHandle),
    privateKeyPem,
    backupEligible: passkeyBoolean(entry, PasskeyField.BackupEligible),
    backupState: passkeyBoolean(entry, PasskeyField.BackupState),
  };
}

export interface KeePassXCPasskeyFile {
  relyingParty: string;
  url: string;
  username: string;
  credentialId: string;
  userHandle: string;
  privateKey: string;
}

export function parsePasskeyFile(value: string): KeePassXCPasskeyFile {
  const parsed: unknown = JSON.parse(value);
  if (!parsed || typeof parsed !== 'object') {
    throw new TypeError('Passkey file must be an object');
  }
  const record = parsed as Record<string, unknown>;
  for (const key of [
    'relyingParty',
    'url',
    'username',
    'credentialId',
    'userHandle',
    'privateKey',
  ]) {
    if (typeof record[key] !== 'string') {
      throw new TypeError(`Invalid passkey property: ${key}`);
    }
  }
  if (!isBase64Url(record.credentialId as string) || !isBase64Url(record.userHandle as string)) {
    throw new TypeError('Passkey identifiers must use Base64URL');
  }
  if (!isPrivateKeyPem(record.privateKey as string)) {
    throw new TypeError('Invalid passkey private key');
  }
  return record as unknown as KeePassXCPasskeyFile;
}

export function serializePasskeyFile(passkey: KeePassXCPasskey, url: string): string {
  return JSON.stringify(
    {
      relyingParty: passkey.relyingParty,
      url,
      username: passkey.username,
      credentialId: passkey.credentialId,
      userHandle: passkey.userHandle,
      privateKey: passkey.privateKeyPem,
    } satisfies KeePassXCPasskeyFile,
    null,
    2,
  );
}

function parseKeeTrayTotp(entry: KdbxEntry): TotpConfig | undefined {
  const seed = entry.get('TOTP Seed');
  if (!seed) {
    return undefined;
  }
  const parts = (entry.get('TOTP Settings') || '30;6').split(';');
  const period = parsePositiveInteger(parts[0], 30);
  const length = parts[1] || '6';
  if (period > 60 || !['6', '8', 'S'].includes(length)) {
    return undefined;
  }
  return {
    source: 'keetraytotp',
    secret: decodeBase32(seed.replace(/ /g, '')),
    period,
    digits: length === '8' ? 8 : length === '6' ? 6 : 5,
    algorithm: 'SHA1',
    encoder: length === 'S' ? 'steam' : 'rfc6238',
    timeCorrectionUrl:
      parts[2]?.startsWith('http://') || parts[2]?.startsWith('https://') ? parts[2] : undefined,
  };
}

function parseOtpField(entry: KdbxEntry): TotpConfig | undefined {
  const value = entry.get('otp');
  if (!value) {
    return undefined;
  }
  if (value.startsWith('otpauth://totp/')) {
    let url: URL;
    try {
      url = new URL(value);
    } catch {
      return undefined;
    }
    const secret = url.searchParams.get('secret');
    if (!secret) {
      return undefined;
    }
    const algorithm = normalizeAlgorithm(url.searchParams.get('algorithm'));
    return {
      source: 'otpauth',
      secret: decodeBase32(secret),
      period: parsePositiveInteger(url.searchParams.get('period'), 30),
      digits: parsePositiveInteger(url.searchParams.get('digits'), 6),
      algorithm,
      encoder: url.searchParams.get('encoder') === 'steam' ? 'steam' : 'rfc6238',
      issuer: url.searchParams.get('issuer') ?? undefined,
      account: decodeURIComponent(url.pathname.slice(1)),
    };
  }
  const parameters = new URLSearchParams(value.replace(/%3d/g, '='));
  const secret = parameters.get('key');
  if (!secret) {
    return undefined;
  }
  return {
    source: 'keeotp',
    secret: decodeBase32(secret),
    period: parsePositiveInteger(parameters.get('step'), 30),
    digits: parsePositiveInteger(parameters.get('size'), 6),
    algorithm: 'SHA1',
    encoder: 'rfc6238',
  };
}

function parseKeePass2Totp(entry: KdbxEntry): TotpConfig | undefined {
  let secret: Uint8Array | undefined;
  if (entry.fields.has('TimeOtp-Secret')) {
    secret = new TextEncoder().encode(entry.get('TimeOtp-Secret'));
  } else if (entry.fields.has('TimeOtp-Secret-Hex')) {
    const value = entry.get('TimeOtp-Secret-Hex');
    if (!/^(?:[\da-f]{2})+$/i.test(value)) {
      return undefined;
    }
    secret = Uint8Array.from(value.match(/.{2}/g)!, part => Number.parseInt(part, 16));
  } else if (entry.fields.has('TimeOtp-Secret-Base32')) {
    secret = decodeBase32(entry.get('TimeOtp-Secret-Base32'));
  } else if (entry.fields.has('TimeOtp-Secret-Base64')) {
    try {
      secret = base64ToBytes(entry.get('TimeOtp-Secret-Base64'));
    } catch {
      return undefined;
    }
  }
  if (!secret) {
    return undefined;
  }
  return {
    source: 'keepass2',
    secret,
    period: parsePositiveInteger(entry.get('TimeOtp-Period'), 30),
    digits: parsePositiveInteger(entry.get('TimeOtp-Length'), 6),
    algorithm: normalizeAlgorithm(entry.get('TimeOtp-Algorithm')),
    encoder: 'rfc6238',
  };
}

function decodeBase32(value: string): Uint8Array {
  const normalized = value.toUpperCase().replace(/=+$/, '');
  if (!/^[A-Z2-7]+$/.test(normalized)) {
    throw new TypeError('Invalid Base32 value');
  }
  const output: number[] = [];
  let bits = 0;
  let buffer = 0;
  for (const character of normalized) {
    const part = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'.indexOf(character);
    buffer = (buffer << 5) | part;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      output.push((buffer >>> bits) & 0xff);
    }
  }
  return Uint8Array.from(output);
}

function normalizeAlgorithm(value: string | null): TotpAlgorithm {
  const normalized = value?.toUpperCase().replaceAll('-', '');
  if (normalized === 'SHA256' || normalized === 'HMACSHA256') {
    return 'SHA256';
  }
  if (normalized === 'SHA512' || normalized === 'HMACSHA512') {
    return 'SHA512';
  }
  return 'SHA1';
}

function webCryptoHash(value: TotpAlgorithm): string {
  if (value === 'SHA1') {
    return 'SHA-1';
  }
  return value === 'SHA256' ? 'SHA-256' : 'SHA-512';
}

function parsePositiveInteger(value: string | null | undefined, fallback: number): number {
  const parsed = Number.parseInt(value ?? '', 10);
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : fallback;
}

function urlFieldIndex(value: string): number {
  if (value === 'KP2A_URL') {
    return 0;
  }
  return Number.parseInt(value.slice('KP2A_URL_'.length), 10) || Number.MAX_SAFE_INTEGER;
}

function passkeyBoolean(entry: KdbxEntry, field: string): boolean {
  if (!entry.fields.has(field)) {
    return true;
  }
  const value = entry.get(field);
  return value === '1' || value === 'true';
}

function isBase64Url(value: string): boolean {
  return /^[\w-]+={0,2}$/.test(value) && !/[+/]/.test(value);
}

function normalizeBase64Url(value: string): string {
  return value.replace(/=+$/, '');
}

function isPrivateKeyPem(value: string): boolean {
  return /^-----BEGIN PRIVATE KEY-----\r?\n[\s\S]+\r?\n-----END PRIVATE KEY-----$/.test(value);
}
