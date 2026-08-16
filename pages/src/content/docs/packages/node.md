---
title: Node.js
description: Install a generated client package or the runtime, open a session, and call a target's ops from Node.js.
---

## Install a client

```bash
npm install @proofofbot/client-altcha
```

The `wred` binary for your platform arrives as an optional dependency. Node 18 or later.

```js
import { AltchaClient } from "@proofofbot/client-altcha";

const client = await AltchaClient.open({});
const result = await client.solve({ url: "https://acme.example/" });
console.log(result);
await client.close();
```

One client owns one session, which owns the mounted realm. Open it once and reuse it. Opening one per call pays the warmup cost every time.

## Install the runtime

Use the runtime to drive a binary that has no generated package, or several targets from one process.

```bash
npm install @proofofbot/runtime
```

```js
import { connect, WreError, ErrorKind } from "@proofofbot/runtime";

const sidecar = await connect({ binary: "/path/to/wred" });
console.log(sidecar.hello.schema_hash);

const session = await sidecar.open("example", {});

try {
  const result = await session.call("solve", { url: "https://acme.example/" }, {
    deadlineMs: 20000,
  });
  console.log(result);
} catch (err) {
  if (err instanceof WreError && err.kind === ErrorKind.Blocked) {
    console.error("target reported a block, retryable:", err.retryable);
  }
  throw err;
}

await session.close();
await sidecar.shutdown();
```

One sidecar process serves many sessions. A session owns the expensive state, a mounted realm or a cookie jar, so open one and reuse it.

## Events

A session is not an event emitter. Events are correlated by call id and delivered to a callback, either for the whole connection or for one call:

```js
const client = await AltchaClient.open({}, {
  onEvent: (id, event, data) => console.log(event, data),
});

await client.solve({ url: "https://acme.example/" }, {
  onEvent: (id, event, data) => console.log(event, data),
});
```

Both fire when both are set: the per-call handler first, then the connection handler. A throw inside either is swallowed.

## Deadlines and cancellation

`deadlineMs` travels on the wire, so the sidecar stops the work rather than the caller abandoning a promise. `signal` takes an `AbortSignal` and sends a cancel frame.

```js
const abort = new AbortController();
setTimeout(() => abort.abort(), 5000);

await client.solve({ url: "https://acme.example/" }, {
  deadlineMs: 20000,
  signal: abort.signal,
});
```

A deadline that passes rejects with `kind === "timeout"`, an abort with `kind === "cancelled"`.

## Binary resolution

`connect()` uses the `binary` option if given, otherwise falls back to `resolveBinary()`, which checks `WRE_BINARY` first. Set `WRE_BINARY` to an absolute path to skip the package's shipped or downloaded binary. That path is used as-is, with no hash check.

## Errors

Every rejection from the sidecar is a `WreError` with a stable `kind`. Branch on `kind`, not on `message`. The [kind table](/web-re-toolkit/packages/#error-kinds) lists all nine.

## Sidecar output and diagnostics

The sidecar writes its log to its own stderr, which is discarded by default so a library does not print into a host process. Pass `{ stderr: "inherit" }` to see it, or set `WRE_STDERR=inherit`.

A failed call writes a report and puts its path in `error.detail.diagnostics`. `WRE_DIAG=always` records every call, `WRE_DIAG=off` records none, and `await client.diagnose(true)` writes one on demand. Read one back with `wre client diag <file>`.
