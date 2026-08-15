# wre-runtime

Python client for the [wre sidecar protocol](../../docs/PROTOCOL.md). It spawns the `wred` binary, speaks the length-prefixed frame protocol over its stdio pipes, and exposes sessions and calls as plain Python objects. Pure standard library, no dependencies.

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
