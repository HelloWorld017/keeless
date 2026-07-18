const DATABASE_NAME = 'keeless-app';
const STORE_NAME = 'identity';
const DEVICE_KEY = 'device-key';

let databasePromise: Promise<IDBDatabase> | undefined;

const openDatabase = () => {
  databasePromise ??= new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(DATABASE_NAME, 1);
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains(STORE_NAME)) {
        database.createObjectStore(STORE_NAME);
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error('Failed to open app storage'));
    request.onblocked = () => reject(new Error('App storage upgrade was blocked'));
  });
  return databasePromise;
};

const readValue = async <T>(key: string): Promise<T | undefined> => {
  const database = await openDatabase();
  return new Promise<T | undefined>((resolve, reject) => {
    const request = database.transaction(STORE_NAME).objectStore(STORE_NAME).get(key);
    request.onsuccess = () => resolve(request.result as T | undefined);
    request.onerror = () => reject(request.error ?? new Error('Failed to read app storage'));
  });
};

const writeValue = async (key: string, value: unknown) => {
  const database = await openDatabase();
  await new Promise<void>((resolve, reject) => {
    const transaction = database.transaction(STORE_NAME, 'readwrite');
    transaction.objectStore(STORE_NAME).put(value, key);
    transaction.oncomplete = () => resolve();
    transaction.onerror = () =>
      reject(transaction.error ?? new Error('Failed to write app storage'));
    transaction.onabort = () =>
      reject(transaction.error ?? new Error('App storage write was aborted'));
  });
};

export const loadDeviceKey = async () => {
  const database = await openDatabase();
  return new Promise<Uint8Array>((resolve, reject) => {
    const transaction = database.transaction(STORE_NAME, 'readwrite');
    const store = transaction.objectStore(STORE_NAME);
    const request = store.get(DEVICE_KEY);
    let key: Uint8Array | undefined;

    request.onsuccess = () => {
      const stored = request.result;
      if (stored instanceof ArrayBuffer && stored.byteLength === 32) {
        key = new Uint8Array(stored);
        return;
      }
      key = crypto.getRandomValues(new Uint8Array(32));
      store.put(key.buffer.slice(0), DEVICE_KEY);
    };
    transaction.oncomplete = () => {
      if (key) {
        resolve(key);
      } else {
        reject(new Error('Device identity transaction completed without a key'));
      }
    };
    transaction.onerror = () =>
      reject(transaction.error ?? new Error('Failed to initialize device identity'));
    transaction.onabort = () =>
      reject(transaction.error ?? new Error('Device identity initialization was aborted'));
  });
};

export const loadTrustedCore = (host: string) => readValue<string>(`trusted-core:${host}`);

export const saveTrustedCore = (host: string, bundle: string) =>
  writeValue(`trusted-core:${host}`, bundle);
