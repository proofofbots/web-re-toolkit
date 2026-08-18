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

Warm a session against a protected page, read the antiforgery token out of the page the session already loaded, and post the site's own search form through the same jar. This is the Lee County court records site, which is Akamai protected end to end: the search answers a session it does not believe with an access denied page or an adaptive challenge, and answers one it does with the case.

```python
import os
import re

from wre_client_akamai import open_client

PAGE = "https://matrix.leeclerk.org/"
SEARCH = "https://matrix.leeclerk.org/Home/SearchByCaseNumber"
CASE = os.environ.get("CASE", "20tr456")


def rows(html: str) -> list[list[str]]:
    body = html.split("<tbody>")[1].split("</tbody>")[0] if "<tbody>" in html else ""

    return [
        [re.sub(r"\s+", " ", re.sub(r"<[^>]*>", "", cell)).strip()
         for cell in re.findall(r"<td[^>]*>(.*?)</td>", row, re.S)]
        for row in re.findall(r"<tr[^>]*>(.*?)</tr>", body, re.S)
    ]


with open_client({"page_url": PAGE}) as client:
    solved = client.solve({})
    print(f"session {solved['run']['machine']}, _abck {solved['cookies']['abck']['status']}")

    page = client.page()
    token = page["fields"].get("__RequestVerificationToken")
    if not token:
        raise SystemExit("the page carries no antiforgery token")

    answer = client.request({
        "url": SEARCH,
        "method": "POST",
        "kind": "form",
        "form": {
            "__RequestVerificationToken": token,
            "byCaseNumber.CaseNumber": CASE,
            "byCaseNumber.CitationNumber": "",
            "searchButton": "SearchByCaseNumber",
        },
    })

    found = rows(answer["body"])
    print(f"search {answer['status']}, refused {answer['refused']}, {len(found)} matching")

    for number, citation, kind, status, filed, *rest in found[:5]:
        print(f"  {number}  {citation}  {kind}  {status}  {filed}")
```

`page` returns the document the session last loaded along with every input it declares, so the antiforgery token comes out of the page the sensor ran on rather than out of a second fetch that would carry a different one. `"kind": "form"` sends the request the way the browser submits that form, headers and all, which is what the edge scores. `refused` is true on a 403, a 429, an access denied body or a challenge redirect.

`discover` reports the surface without running the sensor, so it is the cheapest way to tell whether a page is protected at all.

Events, deadlines, errors and diagnostics work the same for every target and are covered on the [Python package page](/web-re-toolkit/packages/python/). What the client does and what the config controls is in [The Akamai client](/web-re-toolkit/guides/akamai/).
