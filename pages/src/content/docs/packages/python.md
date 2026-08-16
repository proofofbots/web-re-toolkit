---
title: Python
description: Install a generated client package or the runtime, open a session, and call a target's ops from Python, sync or async.
---

## Install a client

```bash
pip install wre-client-altcha
```

Python 3.9 or later. The `wred` binary ships in the wheel for your platform.

```python
from wre_client_altcha import AltchaClient

with AltchaClient.open() as client:
    print(client.solve({"url": "https://acme.example/"}))
```

The client owns one session, which owns the mounted realm. Keep it open and reuse it rather than opening one per call.

For asyncio, wrap the calls with `asyncio.to_thread`, or use `wre_runtime.aio.AsyncSidecar` and attach with `AltchaClient.attach`.

## Install the runtime

Use the runtime to drive a binary that has no generated package, or several targets from one process. Pure standard library, no dependencies.

```bash
pip install wre-runtime
```

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

`connect()` resolves the binary automatically when `binary` is omitted, checking `WRE_BINARY` first. Set `WRE_BINARY` to an absolute path to use a local build. That path skips the SHA-256 check.

## Async

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

## Errors

Every failure raises a `WreError` subclass. Branch on `kind`. The `message` is for humans.

| Kind | Class | Retryable |
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

## Sidecar output and diagnostics

The sidecar writes its log to its own stderr, which is discarded by default so a library does not print into a host process. Pass `stderr="inherit"` to see it. `WRE_STDERR=inherit` or `WRE_STDERR=ignore` overrides the argument, which is the way to turn the log on in a deployed app without touching code.

A failed call writes a report and puts its path in `error.detail["diagnostics"]`. `WRE_DIAG=always` records every call, `WRE_DIAG=off` records none, and `session.call("diag", {"write": True, "events": True})` writes one on demand.
