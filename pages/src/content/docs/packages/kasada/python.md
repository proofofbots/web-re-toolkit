---
title: Kasada from Python
description: Install wre-client-kasada, answer an interrogation, and fetch the page again through the same session.
---

```bash
pip install wre-client-kasada
```

Python 3.9 or later. The `wred` binary ships in the wheel for your platform.

A Kasada session mounts a graph profile. One is compiled into the binary, so there is nothing to capture before the first run. Capture your own with `wre sandbox capture --graph --open` and pass its id as `profile` when you want a graph that is not shared with every other user, or one from a different browser.

```python
from wre_client_kasada import KasadaClient

with KasadaClient.open({"page_url": "https://acme.example/buy"}) as client:
    solved = client.solve({}, deadline=120.0)
    print(solved["verdict"], solved["clearance"])

    page = client.request({"url": "https://acme.example/buy"})
    print(page["status"], page["bytes"])
```

The token is bound to the `KP_UIDz` cookie the edge set on the interrogation, so solve against the url you actually want, then send everything else through the same client.

## A full run

Open one session, report what the page is serving, answer the interrogation, print how many of its own checks the agent flagged, then fetch the page again through the same session and list what came back. A session that never answered gets the interrogation instead of the page, which is the point of the comparison.

```python
import os
import re
import sys

from wre_client_kasada import WreError, open_client

PAGE = os.environ.get("PAGE", "https://www.realestate.com.au/buy/in-sydney,+nsw/list-1")
LISTING = re.compile(r'href="(/property-[^"]+)"')


def listings(html: str) -> list[str]:
    seen: dict[str, None] = {}
    for href in LISTING.findall(html):
        seen.setdefault(href, None)
    return list(seen)


def main() -> int:
    with open_client({"page_url": PAGE}) as client:
        try:
            surface = client.discover({})
            print(f"{PAGE} answered {surface['status']}, protected {surface['protected']}")

            if not surface["protected"]:
                print("no interrogation is being served, nothing to solve")
            else:
                solved = client.solve({}, deadline=120.0)
                print(f"verdict {solved['verdict']}, clearance {solved['clearance']}")
                print(f"payload {solved['payload_bytes']} bytes in {solved['ms']} ms")

                report = client.report()
                print(f"the agent flagged {len(report['flagged'])} of its own checks")

            page = client.request({"url": PAGE}, deadline=60.0)
            print(f"page {page['status']}, {page['bytes']} bytes")

            found = listings(page["body"])
            print(f"{len(found)} listings")
            for href in found[:10]:
                print(f"  https://www.realestate.com.au{href}")
        except WreError as error:
            print(f"{error.kind}: {error}", file=sys.stderr)
            return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
```

`examples/python/kasada_scrape.py` in the repository is the same script written against `wre-runtime` instead of the generated client, which is what you use when you drive several targets from one process.

Events, deadlines, errors and diagnostics work the same for every target and are covered on the [Python package page](/web-re-toolkit/packages/python/). What the client does and what the config controls is in [The Kasada client](/web-re-toolkit/guides/kasada/).
