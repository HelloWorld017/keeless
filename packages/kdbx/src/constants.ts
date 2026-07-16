export const KDBX_SIGNATURE_1 = 0x9aa2d903;
export const KDBX_SIGNATURE_2 = 0xb54bfb67;
export const KDBX_VERSION_4_0 = 0x00040000;
export const KDBX_VERSION_4_1 = 0x00040001;

export const CipherId = {
  Aes256: '31c1f2e6bf714350be5805216afc5aff',
  ChaCha20: 'd6038a2b8b6f4cb5a524339a31dbb59a',
} as const;

export const KdfId = {
  Aes: 'c9d9f39a628a4460bf740d08c18a4fea',
  Argon2d: 'ef636ddf8c29444b91f7a9a403e30a0c',
  Argon2id: '9e298b1956db4773b23dfc3ec6f0a1e6',
} as const;

export const Compression = {
  None: 0,
  GZip: 1,
} as const;

export const InnerStream = {
  None: 0,
  Salsa20: 2,
  ChaCha20: 3,
} as const;

export const StandardField = {
  Title: 'Title',
  UserName: 'UserName',
  Password: 'Password',
  URL: 'URL',
  Notes: 'Notes',
} as const;

export const PasskeyField = {
  Username: 'KPEX_PASSKEY_USERNAME',
  CredentialId: 'KPEX_PASSKEY_CREDENTIAL_ID',
  PrivateKeyPem: 'KPEX_PASSKEY_PRIVATE_KEY_PEM',
  RelyingParty: 'KPEX_PASSKEY_RELYING_PARTY',
  UserHandle: 'KPEX_PASSKEY_USER_HANDLE',
  BackupEligible: 'KPEX_PASSKEY_FLAG_BE',
  BackupState: 'KPEX_PASSKEY_FLAG_BS',
  GeneratedUserId: 'KPEX_PASSKEY_GENERATED_USER_ID',
  CompatibleUsername: 'KPXC_PASSKEY_USERNAME',
} as const;

export const XmlName = {
  Document: 'KeePassFile',
  Meta: 'Meta',
  Root: 'Root',
  Group: 'Group',
  Entry: 'Entry',
  String: 'String',
  Binary: 'Binary',
  Key: 'Key',
  Value: 'Value',
  UUID: 'UUID',
  Times: 'Times',
  History: 'History',
  DeletedObjects: 'DeletedObjects',
  DeletedObject: 'DeletedObject',
} as const;
