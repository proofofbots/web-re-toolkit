import os
import re
import sys

from wre_runtime import WreError, connect

PAGE = os.environ.get("PAGE", "https://www.realestate.com.au/buy/in-sydney,+nsw/list-1")
LISTING = re.compile(r'href="(/property-[^"]+)"')


def listings(html: str) -> list[str]:
    seen: dict[str, None] = {}
    for href in LISTING.findall(html):
        seen.setdefault(href, None)
    return list(seen)


def main() -> int:
    with connect() as sidecar, sidecar.open("kasada", {"page_url": PAGE}) as session:
        try:
            surface = session.call("discover", {})
            print(f"{PAGE} answered {surface['status']}, protected {surface['protected']}")

            if not surface["protected"]:
                print("no interrogation is being served, nothing to solve")
            else:
                solved = session.call("solve", {}, deadline=120.0)
                print(f"verdict {solved['verdict']}, clearance {solved['clearance']}")
                print(f"payload {solved['payload_bytes']} bytes in {solved['ms']} ms")

                report = session.call("report", {})
                print(f"the agent flagged {len(report['flagged'])} of its own checks")

            page = session.call("request", {"url": PAGE}, deadline=60.0)
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
