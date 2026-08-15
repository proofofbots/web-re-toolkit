# wre-runtime

Python client for the [wre sidecar protocol](https://github.com/proofofbots/web-re-toolkit/blob/main/docs/PROTOCOL.md). It spawns the `wred` binary, speaks the length-prefixed frame protocol over its stdio pipes, and exposes sessions and calls as plain Python objects. Pure standard library, no dependencies.

Generated client packages depend on this one. Use it directly to drive a binary that has no generated package, or several targets from one process.

## Install

```
pip install wre-runtime
```

## Usage

```python
from wre_runtime import connect

with connect(binary="/path/to/wred") as sidecar:
    print(sidecar.hello)

    with sidecar.open("example", {"headless": True}) as session:
        result = session.call(
            "solve",
            {"url": "https://acme.example/"},
            deadline=20.0,
        )
        print(result)

    print(sidecar.metrics())
```

`connect()` resolves the binary automatically when `binary` is omitted, checking `WRE_BINARY` first. Set `WRE_BINARY` to an absolute path to use a local build; that path skips the sha256 check.

Async code uses the same API through `connect_async`:

```python
async def main() -> None:
    async with connect_async(binary="/path/to/wred") as sidecar:
        async with await sidecar.open("example", {}) as session:
            print(await session.call("solve", {"url": "https://acme.example/"}))
```

## Events

A session is not an emitter. Events are correlated by call id and delivered to a callback, either for the whole connection or for one call:

```python
with connect(on_event=lambda call_id, event, data: print(call_id, event, data)) as sidecar:
    with sidecar.open("example", {}) as session:
        session.call(
            "solve",
            {"url": "https://acme.example/"},
            on_event=lambda call_id, event, data: print(event, data),
        )
```

Both fire when both are set. The callback runs on the reader thread, so keep it short and do not call back into the session from it.

## Deadlines

`deadline` is in seconds and travels on the wire, so the sidecar stops the work instead of the caller walking away from it. Past the deadline the call raises `Timeout`.

## Sidecar output

The sidecar writes its log to its own stderr, which is discarded by default so a library does not print into a host process. Pass `stderr="inherit"` to see it. `WRE_STDERR=inherit` or `WRE_STDERR=ignore` overrides the argument, which is the way to turn the log on in a deployed app without touching code.

## Errors

Every failure raises a `WreError` subclass. Branch on `kind`; `message` is for humans.

| kind | class | retryable |
| --- | --- | --- |
| `bad_input` | `BadInput` | no |
| `unsupported` | `Unsupported` | no |
| `target_drift` | `TargetDrift` | no |
| `blocked` | `Blocked` | yes |
| `timeout` | `Timeout` | yes |
| `cancelled` | `Cancelled` | no |
| `resource` | `ResourceError` | yes |
| `protocol` | `ProtocolError` | no |
| `internal` | `InternalError` | no |

`error.retryable` reflects what the host sent for that specific failure and can differ from the table.

## Diagnostics

A failed call writes a report and puts its path in `error.detail["diagnostics"]`. `WRE_DIAG=always` records every call, `WRE_DIAG=off` records none, and `session.call("diag", {"write": True, "events": True})` writes one on demand.
