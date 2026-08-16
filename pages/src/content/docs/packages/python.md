---
title: Python
description: Install a generated client package or the runtime, open a session, and call a target's ops from Python, sync or async.
---

## Install a client

```bash
pip install wre-client-akamai
```

Python 3.9 or later. The `wred` binary ships in the wheel for your platform.

```python
from wre_client_akamai import AkamaiClient

with AkamaiClient.open({"page_url": "https://acme.example/"}) as client:
    solved = client.solve({})
    print(solved["cookies"])

    answered = client.request({
        "url": "https://acme.example/api/checkout",
        "method": "POST",
        "json": {"sku": "A-1"},
        "telemetry": True,
    })
    print(answered["status"], answered["refused"])
```

The client owns one session, which owns the mounted realm and the cookie jar. Keep it open and reuse it rather than opening one per call.

For asyncio, wrap the calls with `asyncio.to_thread`, or use `wre_runtime.aio.AsyncSidecar` and attach with `AkamaiClient.attach`.

## A full run

Warm a session against a protected login page, read the antiforgery token out of the page the session already loaded, and post a form through the same jar.

```python
import time
from typing import Optional

from wre_client_akamai import open_client

PAGE = "https://login.xero.com/identity/user/login"
PRECHECK = "https://login.xero.com/identity/user/login/pre-check"


def field(html: str, name: str) -> Optional[str]:
    at = html.find(f'name="{name}"')
    if at < 0:
        return None
    rest = html[at:]
    start = rest.find('value="')
    if start < 0:
        return None
    tail = rest[start + 7 :]
    end = tail.find('"')
    return None if end < 0 else tail[:end]


with open_client({"page_url": PAGE, "wait_ms": 100, "rounds": 1}) as client:
    found = client.discover({})
    print("discover:", {"status": found["status"], "protected": found["protected"]})

    solved = client.solve({})
    print("solve:", {
        "payload_bytes": len(solved.get("payload") or ""),
        "posts": solved["posts"],
    })

    page = client.page()
    html = page["html"] or client.request({"url": PAGE})["body"]
    fields = page["fields"]

    token = fields.get("__RequestVerificationToken") or field(html, "__RequestVerificationToken")
    return_url = fields.get("ReturnUrl") or field(html, "ReturnUrl") or ""
    if not token:
        raise RuntimeError("no antiforgery token")

    username = f"nx{int(time.time()):x}@example.com"

    client.request({
        "url": PRECHECK,
        "method": "POST",
        "json": {"Username": username},
        "headers": {
            "accept": "application/json, text/plain, */*",
            "origin": "https://login.xero.com",
            "requestverificationtoken": token,
        },
    })

    answer = client.request({
        "url": PAGE,
        "method": "POST",
        "form": {
            "ReturnUrl": return_url,
            "PreCheckCompleted": "true",
            "Username": username,
            "Password": "Nx7!aQ2zR9kL",
            "__RequestVerificationToken": token,
        },
        "headers": {
            "accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            "origin": "https://login.xero.com",
            "sec-fetch-dest": "document",
            "sec-fetch-mode": "navigate",
            "sec-fetch-site": "same-origin",
            "upgrade-insecure-requests": "1",
        },
    })

    body = (answer["body"] or "").lower()
    print("login:", {
        "status": answer["status"],
        "refused": answer["refused"],
        "credential_error": "email address or password" in body or "incorrect" in body,
    })
```

`discover` reports the surface without running the sensor, so it is the cheapest way to tell whether a page is protected. `page` returns the document the session last loaded along with every input it declares, which saves a second fetch. `refused` is true on a 403, a 429, an access denied body or a challenge redirect, so a `False` there with a credential error in the body means the session passed and the login itself was rejected.

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
