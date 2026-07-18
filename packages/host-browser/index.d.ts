export default function init(moduleOrPath?: unknown): Promise<unknown>;

export type BrowserDatabase = {
  path: string;
  name: string;
  size: number;
};

export class BrowserCore {
  static create(defaultApprovedBundle?: string | null): Promise<BrowserCore>;
  processFrame(frame: Uint8Array): Promise<Uint8Array | undefined>;
  importDatabase(fileName: string, bytes: Uint8Array): Promise<string>;
  listDatabases(): Promise<BrowserDatabase[]>;
}
