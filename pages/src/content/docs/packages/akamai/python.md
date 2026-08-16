---
title: Akamai from Python
description: Install wre-client-akamai, warm a session against a protected page, and post a form through the same cookie jar.
---

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

Events, deadlines, errors and diagnostics work the same for every target and are covered on the [Python package page](/web-re-toolkit/packages/python/). What the client does and what the config controls is in [The Akamai client](/web-re-toolkit/guides/akamai/).
