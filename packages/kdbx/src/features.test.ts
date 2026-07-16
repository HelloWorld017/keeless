import { describe, expect, it } from 'vitest';
import { generateTotp, getEntryUrls, hasPasskey, parsePasskey, parseTotp } from './features.ts';
import { KdbxEntry } from './model.ts';

describe('KeePass entry features', () => {
  it('reads primary and indexed KP2A URLs in numeric order', () => {
    const entry = new KdbxEntry();
    entry.url = 'https://primary.example';
    entry.set('KP2A_URL_10', 'https://ten.example');
    entry.set('KP2A_URL_2', 'https://two.example');
    entry.set('KP2A_URL', 'https://zero.example');

    expect(getEntryUrls(entry).map(({ field }) => field)).toEqual([
      'URL',
      'KP2A_URL',
      'KP2A_URL_2',
      'KP2A_URL_10',
    ]);
  });

  it('parses KeePass TOTP fields and matches RFC 6238', async () => {
    const entry = new KdbxEntry();
    entry.set('TimeOtp-Secret', '12345678901234567890', { protected: true });
    entry.set('TimeOtp-Period', '30');
    entry.set('TimeOtp-Length', '8');
    entry.set('TimeOtp-Algorithm', 'HMAC-SHA-1');

    const config = parseTotp(entry);

    expect(config?.source).toBe('keepass2');
    await expect(generateTotp(config!, 59_000)).resolves.toBe('94287082');
  });

  it('reads a complete KeePassXC passkey', () => {
    const entry = new KdbxEntry();
    entry.set('KPEX_PASSKEY_USERNAME', 'alice');
    entry.set('KPEX_PASSKEY_CREDENTIAL_ID', 'AQID', { protected: true });
    entry.set('KPEX_PASSKEY_RELYING_PARTY', 'example.com');
    entry.set('KPEX_PASSKEY_USER_HANDLE', 'BAUG', { protected: true });
    entry.set(
      'KPEX_PASSKEY_PRIVATE_KEY_PEM',
      '-----BEGIN PRIVATE KEY-----\nAQID\n-----END PRIVATE KEY-----',
      { protected: true },
    );
    entry.set('KPEX_PASSKEY_FLAG_BE', '1');
    entry.set('KPEX_PASSKEY_FLAG_BS', 'false');

    expect(hasPasskey(entry)).toBe(true);
    expect(parsePasskey(entry)).toMatchObject({
      username: 'alice',
      credentialId: 'AQID',
      relyingParty: 'example.com',
      userHandle: 'BAUG',
      backupEligible: true,
      backupState: false,
    });
  });
});
