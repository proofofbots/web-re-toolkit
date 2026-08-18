---
title: Akamai from Node.js
description: Install @proofofbot/client-akamai, warm a session against a protected page, and post a form through the same cookie jar.
---

```bash
npm install @proofofbot/client-akamai
```

Node 18 or later. The `wred` binary for your platform arrives as an optional dependency.

```js
import { AkamaiClient } from "@proofofbot/client-akamai";

const client = await AkamaiClient.open({ page_url: "https://acme.example/" });

const solved = await client.solve({});
console.log(solved.cookies);

const answered = await client.request({
  url: "https://acme.example/api/checkout",
  method: "POST",
  json: { sku: "A-1" },
  telemetry: true,
});
console.log(answered.status, answered.refused);

await client.close();
```

One client owns one session, which owns the mounted realm and the cookie jar. Open it once and reuse it. Opening one per call pays the warmup cost every time, and throws away the `_abck` cookie the previous run earned.

## A full run

Warm a session against a protected page, read the antiforgery token out of the page the session already loaded, and post the site's own search form through the same jar. This is the Lee County court records site, which is Akamai protected end to end: the search answers a session it does not believe with an access denied page or an adaptive challenge, and answers one it does with the case.

```js
import { open } from "@proofofbot/client-akamai";

const PAGE = "https://matrix.leeclerk.org/";
const SEARCH = "https://matrix.leeclerk.org/Home/SearchByCaseNumber";
const CASE = process.env.CASE ?? "20tr456";

const rows = (html) => {
  const body = html.split("<tbody>")[1]?.split("</tbody>")[0] ?? "";

  return [...body.matchAll(/<tr[^>]*>([\s\S]*?)<\/tr>/g)].map((row) =>
    [...row[1].matchAll(/<td[^>]*>([\s\S]*?)<\/td>/g)].map((cell) =>
      cell[1].replace(/<[^>]*>/g, "").replace(/\s+/g, " ").trim()),
  );
};

const client = await open({ page_url: PAGE });

try {
  const solved = await client.solve({});
  console.log(`session ${solved.run.machine}, _abck ${solved.cookies.abck.status}`);

  const page = await client.page();
  const token = page.fields.__RequestVerificationToken;
  if (!token) throw new Error("the page carries no antiforgery token");

  const answer = await client.request({
    url: SEARCH,
    method: "POST",
    kind: "form",
    form: {
      __RequestVerificationToken: token,
      "byCaseNumber.CaseNumber": CASE,
      "byCaseNumber.CitationNumber": "",
      searchButton: "SearchByCaseNumber",
    },
  });

  const found = rows(answer.body);
  console.log(`search ${answer.status}, refused ${answer.refused}, ${found.length} matching`);

  for (const [number, citation, kind, status, filed] of found.slice(0, 5)) {
    console.log(`  ${number}  ${citation}  ${kind}  ${status}  ${filed}`);
  }
} finally {
  await client.close();
}
```

`page` returns the document the session last loaded along with every input it declares, so the antiforgery token comes out of the page the sensor ran on rather than out of a second fetch that would carry a different one. `kind: "form"` sends the request the way the browser submits that form, headers and all, which is what the edge scores. `refused` is true on a 403, a 429, an access denied body or a challenge redirect.

`discover` reports the surface without running the sensor, so it is the cheapest way to tell whether a page is protected at all.

Events, deadlines, binary resolution and diagnostics work the same for every target and are covered on the [Node.js package page](/web-re-toolkit/packages/node/). What the client does and what the config controls is in [The Akamai client](/web-re-toolkit/guides/akamai/).
