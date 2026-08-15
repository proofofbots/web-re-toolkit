export const ErrorKind = Object.freeze({
  BadInput: "bad_input",
  Unsupported: "unsupported",
  TargetDrift: "target_drift",
  Blocked: "blocked",
  Timeout: "timeout",
  Cancelled: "cancelled",
  Resource: "resource",
  Protocol: "protocol",
  Internal: "internal",
});

export class WreError extends Error {
  constructor(kind, message, extra = {}) {
    super(message);
    this.name = "WreError";
    this.kind = kind;
    this.retryable = extra.retryable ?? false;
    this.target = extra.target ?? undefined;
    this.op = extra.op ?? undefined;
    this.detail = extra.detail ?? undefined;
    if (Error.captureStackTrace) {
      Error.captureStackTrace(this, WreError);
    }
  }

  static isWreError(value) {
    return value instanceof WreError;
  }
}

export function errorFromWire(payload) {
  const source = payload && typeof payload === "object" ? payload : {};
  const kind = typeof source.kind === "string" ? source.kind : ErrorKind.Internal;
  const message = typeof source.message === "string" ? source.message : "unknown error";
  return new WreError(kind, message, {
    retryable: typeof source.retryable === "boolean" ? source.retryable : false,
    target: typeof source.target === "string" ? source.target : undefined,
    op: typeof source.op === "string" ? source.op : undefined,
    detail: source.detail,
  });
}
