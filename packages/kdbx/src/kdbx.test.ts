import { describe, expect, it } from 'vitest';
import { CipherId, Compression } from './constants.ts';
import { KdbxCredentials } from './crypto.ts';
import { Kdbx, loadKdbx, saveKdbx } from './kdbx.ts';
import { KdbxBinary, KdbxEntry } from './model.ts';

describe('KDBX4', () => {
  it('round-trips an editable AES database with protected values and binaries', async () => {
    const database = createTestDatabase();
    const credentials = new KdbxCredentials({ password: 'correct horse battery staple' });
    const group = database.root.addGroup('Websites');
    const entry = group.addEntry();
    entry.title = 'Example';
    entry.username = 'alice';
    entry.password = 'very secret';
    entry.url = 'https://example.com';
    entry.tags = ['personal', 'login'];
    entry.set('Custom', 'custom value', { protected: true });
    entry.binaries.set('hello.txt', new KdbxBinary(new TextEncoder().encode('hello')));
    entry.qualityCheck = false;
    entry.customData.set('entry-secret', { value: 'hidden', protected: true });
    entry.autoTypeAssociations.push({
      window: '*Example*',
      keystrokeSequence: '{USERNAME}{TAB}{PASSWORD}',
    });
    group.tags = ['work'];
    group.customData.set('group-setting', { value: 'enabled' });
    const customIconUuid = entry.uuid;
    entry.customIconUuid = customIconUuid;
    database.meta.customIcons.push({
      uuid: customIconUuid,
      data: Uint8Array.of(1, 2, 3),
      name: 'Example icon',
    });
    database.meta.customData.set('meta-secret', { value: 'metadata', protected: true });

    const encoded = await saveKdbx(database, credentials);
    const decoded = await loadKdbx(encoded, credentials);
    const decodedEntry = decoded.root.groups[0]?.entries[0];

    expect(new DataView(encoded.buffer, encoded.byteOffset).getUint32(8, true)).toBe(0x00040001);
    expect(decoded.meta.databaseName).toBe('Test database');
    expect(decoded.root.groups[0]?.name).toBe('Websites');
    expect(decodedEntry?.title).toBe('Example');
    expect(decodedEntry?.password).toBe('very secret');
    expect(decodedEntry?.fields.get('Password')?.protected).toBe(true);
    expect(decodedEntry?.get('Custom')).toBe('custom value');
    expect(decodedEntry?.tags).toEqual(['personal', 'login']);
    expect(decodedEntry?.binaries.get('hello.txt')?.data).toEqual(
      new TextEncoder().encode('hello'),
    );
    expect(decodedEntry?.qualityCheck).toBe(false);
    expect(decodedEntry?.customData.get('entry-secret')).toMatchObject({
      value: 'hidden',
      protected: true,
    });
    expect(decodedEntry?.autoTypeAssociations).toEqual([
      { window: '*Example*', keystrokeSequence: '{USERNAME}{TAB}{PASSWORD}' },
    ]);
    expect(decoded.root.groups[0]?.tags).toEqual(['work']);
    expect(decoded.root.groups[0]?.customData.get('group-setting')?.value).toBe('enabled');
    expect(decoded.meta.customIcons[0]).toMatchObject({ name: 'Example icon' });
    expect(decoded.meta.customIcons[0]?.data).toEqual(Uint8Array.of(1, 2, 3));
    expect(decoded.meta.customData.get('meta-secret')).toMatchObject({
      value: 'metadata',
      protected: true,
    });
  });

  it('keeps the protected stream aligned for empty field names', async () => {
    const database = createTestDatabase();
    const entry = database.root.addEntry();
    entry.fields.clear();
    entry.set('', 'first', { protected: true });
    entry.set('after', 'second', { protected: true });

    const encoded = await Kdbx.save(database, 'password');
    const decoded = await Kdbx.load(encoded, 'password');

    expect(decoded.root.entries[0]?.get('')).toBe('first');
    expect(decoded.root.entries[0]?.get('after')).toBe('second');
  });

  it('round-trips ChaCha20 without compression through the facade API', async () => {
    const database = createTestDatabase();
    database.root.addEntry(new KdbxEntry()).title = 'ChaCha';

    const encoded = await Kdbx.save(database, 'password', {
      cipher: CipherId.ChaCha20,
      compression: Compression.None,
    });
    const decoded = await Kdbx.load(encoded, 'password');

    expect(decoded.root.entries[0]?.title).toBe('ChaCha');
    expect(decoded.header.cipher).toBe(CipherId.ChaCha20);
    expect(decoded.header.compression).toBe(Compression.None);
  });

  it('rejects wrong credentials and modified headers', async () => {
    const database = createTestDatabase();
    const encoded = await Kdbx.save(database, 'right password');

    await expect(Kdbx.load(encoded, 'wrong password')).rejects.toMatchObject({
      code: 'invalid-credentials',
    });

    const corrupted = encoded.slice();
    corrupted[20] ^= 1;
    await expect(Kdbx.load(corrupted, 'right password')).rejects.toMatchObject({
      code: 'corrupt-data',
    });
  });

  it('rejects KDF parameters above the configured resource limits', async () => {
    const database = createTestDatabase();
    database.header.kdfParameters.set('M', 257n * 1024n * 1024n);

    await expect(Kdbx.save(database, 'password')).rejects.toThrow(/resource limit/);
  });
});

function createTestDatabase() {
  const database = Kdbx.create('Test database');
  database.header.kdfParameters.set('I', 1n);
  database.header.kdfParameters.set('M', 8n * 1024n * 1024n);
  database.header.kdfParameters.set('P', 1);
  return database;
}
