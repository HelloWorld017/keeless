export type KdbxErrorCode =
  | 'invalid-signature'
  | 'unsupported-version'
  | 'unsupported-cipher'
  | 'unsupported-kdf'
  | 'invalid-header'
  | 'invalid-credentials'
  | 'corrupt-data'
  | 'invalid-xml'
  | 'invalid-key-file';

export class KdbxError extends Error {
  readonly code: KdbxErrorCode;

  constructor(code: KdbxErrorCode, message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = 'KdbxError';
    this.code = code;
  }
}
