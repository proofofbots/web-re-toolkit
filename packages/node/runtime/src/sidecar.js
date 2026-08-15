import { spawn } from "node:child_process";
import { encodeFrame, FrameDecoder } from "./frames.js";
import { WreError, ErrorKind, errorFromWire } from "./errors.js";
import { resolveBinary } from "./binary.js";

const CANCEL_GRACE_MS = 5000;
const SHUTDOWN_CAP_MS = 5000;

export async function connect(options = {}) {
  const binary = options.binary ?? resolveBinary();
  const args = options.args ?? [];
  const env = { ...process.env, ...(options.env ?? {}) };
  const stderrMode = options.stderr === "ignore" ? "ignore" : "inherit";
  const expectProtocol = options.expectProtocol ?? 1;
  const expectSchemaHash = options.expectSchemaHash;
  const startupTimeoutMs = options.startupTimeoutMs ?? 30000;

  const child = spawn(binary, ["--stdio", ...args], {
    cwd: options.cwd,
    env,
    stdio: ["pipe", "pipe", stderrMode],
  });

  const sidecar = new Sidecar(child, options.onEvent);

  let startupTimer;
  const startupTimeout = new Promise((_resolve, reject) => {
    startupTimer = setTimeout(() => {
      reject(new WreError(ErrorKind.Timeout, `sidecar did not respond to hello within ${startupTimeoutMs}ms`, { retryable: true }));
    }, startupTimeoutMs);
  });

  const onEarlySpawnError = (err) => {
    sidecar._failAll(new WreError(ErrorKind.Resource, `failed to spawn sidecar: ${err.message}`));
  };
  child.once("error", onEarlySpawnError);

  try {
    const { result: hello } = await Promise.race([sidecar._request("hello", {}), startupTimeout]);
    clearTimeout(startupTimer);
    child.removeListener("error", onEarlySpawnError);

    if (hello.protocol !== expectProtocol) {
      sidecar._forceKill();
      throw new WreError(
        ErrorKind.Protocol,
        `protocol mismatch: expected ${expectProtocol}, sidecar reports ${hello.protocol}`,
      );
    }
    if (expectSchemaHash !== undefined && hello.schema_hash !== expectSchemaHash) {
      sidecar._forceKill();
      throw new WreError(
        ErrorKind.Protocol,
        `schema hash mismatch: expected "${expectSchemaHash}", sidecar reports "${hello.schema_hash}"`,
      );
    }

    sidecar._hello = hello;
    return sidecar;
  } catch (err) {
    clearTimeout(startupTimer);
    child.removeListener("error", onEarlySpawnError);
    sidecar._forceKill();
    throw err;
  }
}

export class Sidecar {
  constructor(child, onEvent) {
    this._child = child;
    this._onEvent = typeof onEvent === "function" ? onEvent : undefined;
    this._decoder = new FrameDecoder();
    this._pending = new Map();
    this._nextId = 1;
    this._hello = undefined;
    this._closed = false;
    this._exited = false;
    this._exitPromise = new Promise((resolve) => {
      this._resolveExit = resolve;
    });

    child.stdout.on("data", (chunk) => this._handleChunk(chunk));
    child.stdout.on("end", () => {
      this._failAll(new WreError(ErrorKind.Protocol, "sidecar closed the stream (stdout ended)"));
    });
    child.once("exit", (code, signal) => {
      this._exited = true;
      this._failAll(
        new WreError(ErrorKind.Protocol, `sidecar closed the stream (process exited, code=${code}, signal=${signal})`),
      );
      this._resolveExit();
    });
    child.on("error", (err) => {
      this._failAll(new WreError(ErrorKind.Resource, `sidecar process error: ${err.message}`, { retryable: true }));
    });
    if (child.stdin) {
      child.stdin.on("error", () => {});
    }
  }

  get hello() {
    return this._hello;
  }

  get pid() {
    return this._child.pid;
  }

  async describe() {
    const { result } = await this._request("describe", {});
    return result;
  }

  async targets() {
    const { result } = await this._request("targets", {});
    return result;
  }

  async metrics() {
    const { result } = await this._request("metrics", {});
    return result;
  }

  async open(target, config = {}) {
    const { result } = await this._request("open", { target, config });
    return new Session(this, result);
  }

  async call(op, params = {}, opts = {}) {
    const { result } = await this._request(op, params, opts);
    return result;
  }

  async callWithBinary(op, params = {}, opts = {}) {
    return this._request(op, params, opts);
  }

  async close() {
    if (!this._closed) {
      this._closed = true;
      this._failAll(new WreError(ErrorKind.Protocol, "sidecar connection closed"));
      this._child.kill();
    }
    return this._exitPromise;
  }

  async shutdown() {
    if (!this._closed) {
      await Promise.race([
        this._request("shutdown", {}).catch(() => {}),
        new Promise((resolve) => setTimeout(resolve, SHUTDOWN_CAP_MS)),
      ]);
    }
    return this.close();
  }

  _request(op, params = {}, opts = {}) {
    if (this._closed || this._exited) {
      return Promise.reject(new WreError(ErrorKind.Protocol, "sidecar is closed"));
    }

    const id = this._nextId++;
    const envelope = { t: "req", v: 1, id, op, params };
    if (opts.session) envelope.session = opts.session;
    if (typeof opts.deadlineMs === "number") envelope.deadline_ms = opts.deadlineMs;

    return new Promise((resolve, reject) => {
      const entry = { resolve, reject, onEvent: opts.onEvent, timer: undefined, abortCleanup: undefined };
      this._pending.set(id, entry);

      if (typeof opts.deadlineMs === "number") {
        entry.timer = setTimeout(() => {
          this._cancel(id);
          this._settleReject(
            id,
            new WreError(ErrorKind.Timeout, `op "${op}" exceeded its deadline of ${opts.deadlineMs}ms`, { retryable: true, op }),
          );
        }, opts.deadlineMs + CANCEL_GRACE_MS);
      }

      if (opts.signal) {
        if (opts.signal.aborted) {
          this._pending.delete(id);
          if (entry.timer) clearTimeout(entry.timer);
          reject(new WreError(ErrorKind.Cancelled, `op "${op}" aborted before it was sent`, { op }));
          return;
        }
        const onAbort = () => {
          this._cancel(id);
          this._settleReject(id, new WreError(ErrorKind.Cancelled, `op "${op}" was cancelled`, { op }));
        };
        opts.signal.addEventListener("abort", onAbort, { once: true });
        entry.abortCleanup = () => opts.signal.removeEventListener("abort", onAbort);
      }

      try {
        this._send(envelope, opts.bin);
      } catch (err) {
        this._pending.delete(id);
        if (entry.timer) clearTimeout(entry.timer);
        if (entry.abortCleanup) entry.abortCleanup();
        reject(err instanceof WreError ? err : new WreError(ErrorKind.Protocol, String(err && err.message ? err.message : err)));
      }
    });
  }

  _send(json, bin) {
    if (!this._child.stdin || this._child.stdin.destroyed) {
      throw new WreError(ErrorKind.Protocol, "sidecar stdin is closed");
    }
    this._child.stdin.write(encodeFrame(json, bin));
  }

  _cancel(id) {
    if (!this._pending.has(id)) return;
    try {
      this._send({ t: "cancel", v: 1, id });
    } catch {}
  }

  _settleReject(id, err) {
    const entry = this._pending.get(id);
    if (!entry) return;
    this._pending.delete(id);
    if (entry.timer) clearTimeout(entry.timer);
    if (entry.abortCleanup) entry.abortCleanup();
    entry.reject(err);
  }

  _failAll(err) {
    const entries = Array.from(this._pending.values());
    this._pending.clear();
    for (const entry of entries) {
      if (entry.timer) clearTimeout(entry.timer);
      if (entry.abortCleanup) entry.abortCleanup();
      entry.reject(err);
    }
  }

  _forceKill() {
    if (!this._exited) {
      this._child.kill();
    }
  }

  _handleChunk(chunk) {
    let frames;
    try {
      frames = this._decoder.push(chunk);
    } catch (err) {
      this._failAll(err instanceof WreError ? err : new WreError(ErrorKind.Protocol, String(err && err.message ? err.message : err)));
      this._child.kill();
      return;
    }
    for (const frame of frames) {
      this._handleFrame(frame);
    }
  }

  _handleFrame({ json, bin }) {
    if (json.t === "res") {
      this._handleResponse(json, bin);
    } else if (json.t === "evt") {
      this._handleEvent(json);
    }
  }

  _handleResponse(msg, bin) {
    const entry = this._pending.get(msg.id);
    if (!entry) return;
    this._pending.delete(msg.id);
    if (entry.timer) clearTimeout(entry.timer);
    if (entry.abortCleanup) entry.abortCleanup();
    if (msg.ok) {
      entry.resolve({ result: msg.result, bin });
    } else {
      entry.reject(errorFromWire(msg.error));
    }
  }

  _handleEvent(msg) {
    const entry = this._pending.get(msg.id);
    if (entry && entry.onEvent) {
      try {
        entry.onEvent(msg.id, msg.event, msg.data);
      } catch {}
    }
    if (this._onEvent) {
      try {
        this._onEvent(msg.id, msg.event, msg.data);
      } catch {}
    }
  }
}

export class Session {
  constructor(sidecar, info) {
    this._sidecar = sidecar;
    this._id = info.session;
    this._target = info.target;
    this._ops = Array.isArray(info.ops) ? info.ops : [];
    this._closed = false;
  }

  get id() {
    return this._id;
  }

  get target() {
    return this._target;
  }

  get ops() {
    return this._ops;
  }

  async call(op, params = {}, opts = {}) {
    const { result } = await this._sidecar._request(op, params, { ...opts, session: this._id });
    return result;
  }

  async callWithBinary(op, params = {}, opts = {}) {
    return this._sidecar._request(op, params, { ...opts, session: this._id });
  }

  async warmup() {
    return this.call("warmup", {});
  }

  async health() {
    return this.call("health", {});
  }

  async close() {
    if (this._closed) {
      return { closed: true };
    }
    this._closed = true;
    return this.call("close", {});
  }
}
