# @proofofbot/runtime

Node.js client for the [wre sidecar protocol](https://proofofbots.github.io/web-re-toolkit/reference/protocol/). It spawns the `wred` binary, speaks the length-prefixed frame protocol over its stdin and stdout, and gives you a `Sidecar` for base ops plus `Session` objects for target ops.

Generated client packages depend on this one. Use it directly when you want to drive a binary that has no generated package, or several targets from one process.

## Install

```bash
npm install @proofofbot/runtime
```

## Usage

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
const sidecar = await connect({
  onEvent: (id, event, data) => console.log(id, event, data),
});

await session.call("solve", params, {
  onEvent: (id, event, data) => console.log(event, data),
});
```

Both fire when both are set: the per-call handler first, then the connection handler. A throw inside either is swallowed.

## Deadlines and cancellation

`deadlineMs` travels on the wire, so the sidecar stops the work rather than the caller abandoning a promise. `signal` takes an `AbortSignal` and sends a cancel frame.

```js
const abort = new AbortController();
setTimeout(() => abort.abort(), 5000);

await session.call("solve", params, { deadlineMs: 20000, signal: abort.signal });
```

A deadline that passes rejects with `kind === "timeout"`, an abort with `kind === "cancelled"`.

## Binary resolution

`connect()` uses the `binary` option if given, otherwise falls back to `resolveBinary()`, which checks `WRE_BINARY` first. Set `WRE_BINARY` to an absolute path to a `wred` binary to skip the package's shipped or downloaded binary. That path is used as-is, with no hash check.

## Sidecar output

The sidecar writes its log to its own stderr, which is discarded by default so a library does not print into a host process. Pass `{ stderr: "inherit" }` to see it. `WRE_STDERR=inherit` or `WRE_STDERR=ignore` overrides the option, which is the way to turn the log on in a deployed app without touching code.

## Errors

Every rejection from the sidecar is a `WreError` with a stable `kind`. Branch on `kind`, not on `message`.

| kind | meaning | retryable |
| --- | --- | --- |
| `bad_input` | parameters failed validation against the op schema | no |
| `unsupported` | no such op, target or option in this build | no |
| `target_drift` | the shipped script no longer matches what the client expects | no |
| `blocked` | the service answered with a challenge or a block | yes |
| `timeout` | the deadline passed | yes |
| `cancelled` | the caller cancelled | no |
| `resource` | something the client needs is missing or exhausted | yes |
| `protocol` | malformed frame, envelope or version mismatch | no |
| `internal` | unclassified | no |

`retryable` on the error instance reflects what the host sent on the wire, and can override the default in this table for a specific failure.

## Diagnostics

A failed call writes a report and puts its path in `error.detail.diagnostics`. `WRE_DIAG=always` records every call, `WRE_DIAG=off` records none, and `session.call("diag", { write: true, events: true })` writes one on demand.
