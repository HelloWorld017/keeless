import { describe, expect, it } from 'vitest';
import { BinaryReader, bytesToBase64 } from './binary.ts';
import {
  ChaCha20Stream,
  KdbxCredentials,
  readHmacBlocks,
  sha256,
  writeHmacBlocks,
} from './crypto.ts';
import {
  readVariantDictionary,
  type VariantDictionary,
  type VariantValue,
  writeVariantDictionary,
} from './variant-dictionary.ts';

describe('binary cryptography', () => {
  it('matches the RFC 8439 ChaCha20 block vector', () => {
    const key = Uint8Array.from({ length: 32 }, (_, index) => index);
    const nonce = Uint8Array.of(0, 0, 0, 9, 0, 0, 0, 0x4a, 0, 0, 0, 0);
    const expected =
      '10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4e' +
      'd2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e';

    const result = new ChaCha20Stream(key, nonce, 1).process(new Uint8Array(64));

    expect(toHex(result)).toBe(expected);
  });

  it('authenticates every payload block and its terminator', async () => {
    const key = Uint8Array.from({ length: 64 }, (_, index) => index);
    const value = Uint8Array.from({ length: 37 }, (_, index) => index * 3);
    const encoded = await writeHmacBlocks(value, key, 10);

    await expect(readHmacBlocks(new BinaryReader(encoded), key)).resolves.toEqual(value);

    const tampered = encoded.slice();
    tampered[40] ^= 1;
    await expect(readHmacBlocks(new BinaryReader(tampered), key)).rejects.toMatchObject({
      code: 'corrupt-data',
    });
  });

  it('parses KeePass key-file v1 and v2 encodings', async () => {
    const key = Uint8Array.from({ length: 32 }, (_, index) => index);
    const hash = await sha256(key);
    const expectedCompositeKey = await sha256(key);
    const v1 = new TextEncoder().encode(
      `<KeyFile><Meta><Version>1.00</Version></Meta><Key><Data>${bytesToBase64(key)}</Data></Key></KeyFile>`,
    );
    const v2 = new TextEncoder().encode(
      `<KeyFile><Meta><Version>2.0</Version></Meta><Key><Data Hash="${toHex(hash.subarray(0, 4)).toUpperCase()}">${toHex(key).toUpperCase()}</Data></Key></KeyFile>`,
    );

    await expect(new KdbxCredentials({ keyFile: v1 }).getCompositeKey()).resolves.toEqual(
      expectedCompositeKey,
    );
    await expect(new KdbxCredentials({ keyFile: v2 }).getCompositeKey()).resolves.toEqual(
      expectedCompositeKey,
    );
    await expect(new KdbxCredentials().getCompositeKey()).resolves.toEqual(
      await sha256(new Uint8Array()),
    );

    const invalidV2 = new TextEncoder().encode(
      `<KeyFile><Meta><Version>2.0</Version></Meta><Key><Data>${toHex(key)}</Data></Key></KeyFile>`,
    );
    await expect(
      new KdbxCredentials({ keyFile: invalidV2 }).getCompositeKey(),
    ).rejects.toMatchObject({ code: 'invalid-key-file' });
  });
});

describe('variant dictionary', () => {
  it('round-trips KDF parameter types', () => {
    const dictionary: VariantDictionary = new Map<string, VariantValue>([
      ['$UUID', Uint8Array.from({ length: 16 }, (_, index) => index)],
      ['V', 0x13],
      ['I', 4n],
      ['Enabled', true],
      ['Label', 'Argon2id'],
    ]);

    const result = readVariantDictionary(writeVariantDictionary(dictionary));

    expect(result).toEqual(dictionary);
  });
});

function toHex(value: Uint8Array): string {
  return Array.from(value, byte => byte.toString(16).padStart(2, '0')).join('');
}
