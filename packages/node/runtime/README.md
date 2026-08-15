# @wre/runtime

Node.js client for the [wre sidecar protocol](../../../docs/PROTOCOL.md). It spawns the `wred` binary, speaks the length-prefixed frame protocol over its stdin and stdout, and gives you a `Sidecar` for base ops plus `Session` objects for target ops.

## Install

```
npm install @wre/runtime
```

## Usage

```js
import { connect, WreError, ErrorKind } from "@wre/runtime";

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

## Binary resolution

`connect()` uses the `binary` option if given, otherwise falls back to `resolveBinary()`, which checks `WRE_BINARY` first. Set `WRE_BINARY` to an absolute path to a `wred` binary to skip the package's shipped or downloaded binary. That path is used as-is, with no hash check.

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
