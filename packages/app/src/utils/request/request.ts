import { Client as WireClient } from '@keeless/lesswire';
import type { Host } from '@/types/Host';
import type { ClientStore } from '@keeless/lesswire';
import { KeyScope, operationMetadata, type Operation, type OperationResponse, type OperationSuccess } from '@keeless/schema';

const encoder = new TextEncoder();
const decoder = new TextDecoder(undefined, { fatal: true });

let memoryClientStore: ClientStore;

const createMemoryClientStore = (): ClientStore => {
  const deviceKey = crypto.getRandomValues(new Uint8Array(32));
  const trustedServers = new Map<string, string>();

  return {
    loadDeviceKey: async () => deviceKey.slice(),
    loadTrustedServer: async endpointId => trustedServers.get(endpointId),
    saveTrustedServer: async (endpointId, bundle) => {
      trustedServers.set(endpointId, bundle);
    },
  };
};

export type OperationName = Operation['op'];
export type OperationArgs<TName extends OperationName> = Extract<Operation, { op: TName }>['args'];
export type OperationResult<TName extends OperationName> = Extract<
  OperationSuccess,
  { op: TName }
>['result'];

export class CoreRequestError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = 'CoreRequestError';
    this.code = code;
  }
}

export class RequestClient {
  private constructor(
    readonly host: Host,
    private readonly untrustedWire: WireClient,
    private coreWire?: WireClient,
  ) {}

  static async connect(host: Host) {
    memoryClientStore ??= createMemoryClientStore();
    const recipient = await host.connect();
    return new RequestClient(
      host,
      await WireClient.connect(host, 'app', recipient, memoryClientStore, `${host.id}:untrusted`),
    );
  }

  async request<TName extends OperationName>(
    op: TName,
    args: OperationArgs<TName>,
  ): Promise<OperationResult<TName>> {
    const requestId = crypto.randomUUID();
    const plaintext = encoder.encode(JSON.stringify({ requestId, op, args }));
    let responseBytes: Uint8Array;
    try {
      responseBytes = await this.getWire(op).request(plaintext);
    } finally {
      plaintext.fill(0);
    }
    try {
      const response = JSON.parse(decoder.decode(responseBytes)) as OperationResponse;
      if (typeof __DEV__ === 'boolean' && __DEV__) {
        console.log(`Operation ${requestId}: ${op}(`, args, `) -> `, response);
      }

      if (response.requestId !== requestId) {
        throw new Error('Server returned a mismatched request ID');
      }
      if (response.status === 'error') {
        throw new CoreRequestError(response.error.code, response.error.message);
      }
      if (response.op !== op) {
        throw new Error('Server returned a mismatched operation');
      }
      return response.result as OperationResult<TName>;
    } finally {
      responseBytes.fill(0);
    }
  }

  upload(file: File) {
    return this.getCoreWire().upload(file);
  }

  download(transferId: string) {
    return this.getCoreWire().download(transferId);
  }

  async upgrade() {
    const { publicKey } = await this.request('upgrade', {});
    this.coreWire = await WireClient.connect(
      this.host,
      'app',
      publicKey,
      memoryClientStore,
      `${this.host.id}:core`,
    );
  }

  private getWire(op: OperationName) {
    const isUntrustedOperation = (operationMetadata[op].recipients as readonly KeyScope[]).includes('core_untrusted');
    return isUntrustedOperation ? this.untrustedWire : this.getCoreWire();
  }

  private getCoreWire() {
    if (!this.coreWire) {
      throw new Error('Core connection is not available');
    }
    return this.coreWire;
  }
}

export const getRequestClient = (host: Host) => RequestClient.connect(host);
