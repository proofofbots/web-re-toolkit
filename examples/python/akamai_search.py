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
