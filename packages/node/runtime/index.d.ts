export type ErrorKindValue =
  | "bad_input"
  | "unsupported"
  | "target_drift"
  | "blocked"
  | "timeout"
  | "cancelled"
  | "resource"
  | "protocol"
  | "internal";

export declare const ErrorKind: {
  readonly BadInput: "bad_input";
  readonly Unsupported: "unsupported";
  readonly TargetDrift: "target_drift";
  readonly Blocked: "blocked";
  readonly Timeout: "timeout";
  readonly Cancelled: "cancelled";
  readonly Resource: "resource";
  readonly Protocol: "protocol";
  readonly Internal: "internal";
};

export interface WreErrorExtra {
  retryable?: boolean;
  target?: string;
  op?: string;
  detail?: unknown;
}

export declare class WreError extends Error {
  readonly name: "WreError";
  readonly kind: ErrorKindValue;
  readonly retryable: boolean;
  readonly target: string | undefined;
  readonly op: string | undefined;
  readonly detail: unknown;
  constructor(kind: ErrorKindValue, message: string, extra?: WreErrorExtra);
  static isWreError(value: unknown): value is WreError;
}

export interface WireError {
  kind?: string;
  message?: string;
  retryable?: boolean;
  target?: string;
  op?: string;
  detail?: unknown;
}

export declare function errorFromWire(payload: WireError | null | undefined): WreError;

export interface DecodedFrame {
  json: unknown;
  bin: Buffer;
}

export declare function encodeFrame(jsonValue: unknown, bin?: Buffer | Uint8Array | null): Buffer;

export declare class FrameDecoder {
  push(chunk: Buffer | Uint8Array): DecodedFrame[];
}

export declare function currentTriple(): string;

export interface ResolveBinaryOptions {
  embedded?: string;
  sha256?: string;
}

export declare function resolveBinary(options?: ResolveBinaryOptions): string;

export declare function verifySha256(path: string, expected: string): void;

export interface HelloInfo {
  protocol: number;
  bundle: string;
  binary_version: string;
  toolkit_version: string;
  schema_hash: string;
  targets: string[];
  workers: number;
  pid: number;
}

export type SidecarEventListener = (id: number, event: string, data: unknown) => void;

export interface ConnectOptions {
  binary?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string;
  stderr?: "inherit" | "ignore";
  onEvent?: SidecarEventListener;
  expectProtocol?: number;
  expectSchemaHash?: string;
  startupTimeoutMs?: number;
}

export interface CallOptions {
  session?: string;
  deadlineMs?: number;
  signal?: AbortSignal;
  onEvent?: SidecarEventListener;
  bin?: Buffer | Uint8Array;
}

export interface CallWithBinaryResult {
  result: unknown;
  bin: Buffer;
}

export interface OpenResult {
  session: string;
  target: string;
  worker: number;
  ops: string[];
}

export declare class Session {
  constructor(sidecar: Sidecar, info: OpenResult);
  get id(): string;
  get target(): string;
  get ops(): string[];
  call(op: string, params?: Record<string, unknown>, opts?: CallOptions): Promise<unknown>;
  callWithBinary(op: string, params?: Record<string, unknown>, opts?: CallOptions): Promise<CallWithBinaryResult>;
  warmup(): Promise<unknown>;
  health(): Promise<unknown>;
  close(): Promise<unknown>;
}

export declare class Sidecar {
  constructor(child: import("node:child_process").ChildProcess, onEvent?: SidecarEventListener);
  get hello(): HelloInfo;
  get pid(): number | undefined;
  describe(): Promise<unknown>;
  targets(): Promise<string[]>;
  metrics(): Promise<Record<string, number>>;
  open(target: string, config?: Record<string, unknown>): Promise<Session>;
  call(op: string, params?: Record<string, unknown>, opts?: CallOptions): Promise<unknown>;
  callWithBinary(op: string, params?: Record<string, unknown>, opts?: CallOptions): Promise<CallWithBinaryResult>;
  close(): Promise<void>;
  shutdown(): Promise<void>;
}

export declare function connect(options?: ConnectOptions): Promise<Sidecar>;
