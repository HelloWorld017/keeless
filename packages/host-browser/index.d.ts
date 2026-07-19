export default function init(moduleOrPath?: unknown): Promise<unknown>;

export class BrowserCore {
  static create(defaultApprovedBundle?: string | null): Promise<BrowserCore>;
  handle(frame: Uint8Array): Promise<Uint8Array | undefined>;
  configureWebDav(url: string, username: string, password: string): Promise<void>;
  configureLocalFile(file: File, handle?: FileSystemFileHandle | null): Promise<void>;
}
