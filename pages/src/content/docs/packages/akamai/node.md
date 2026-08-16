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

Warm a session against a protected login page, read the antiforgery token out of the page the session already loaded, and post a form through the same jar.

```js
import { open } from "@proofofbot/client-akamai";

const PAGE = "https://login.xero.com/identity/user/login";
const PRECHECK = "https://login.xero.com/identity/user/login/pre-check";

const field = (html, name) => {
  const at = html.indexOf(`name="${name}"`);
  if (at < 0) return null;
  const rest = html.slice(at);
  const start = rest.indexOf('value="');
  if (start < 0) return null;
  const tail = rest.slice(start + 7);
  const end = tail.indexOf('"');
  return end < 0 ? null : tail.slice(0, end);
};

const client = await open({ page_url: PAGE, wait_ms: 100, rounds: 1 });

try {
  const found = await client.discover({});
  console.log("discover:", { status: found.status, protected: found.protected });

  const solved = await client.solve({});
  console.log("solve:", {
    payload_bytes: solved.payload?.length ?? 0,
    posts: solved.posts,
  });

  const page = await client.page();
  const html = page.html || (await client.request({ url: PAGE })).body;

  const token = page.fields?.__RequestVerificationToken ?? field(html, "__RequestVerificationToken");
  const returnUrl = page.fields?.ReturnUrl ?? field(html, "ReturnUrl") ?? "";
  if (!token) throw new Error("no antiforgery token");

  const username = `nx${Date.now().toString(16)}@example.com`;

  await client.request({
    url: PRECHECK,
    method: "POST",
    json: { Username: username },
    headers: {
      accept: "application/json, text/plain, */*",
      origin: "https://login.xero.com",
      requestverificationtoken: token,
    },
  });

  const answer = await client.request({
    url: PAGE,
    method: "POST",
    form: {
      ReturnUrl: returnUrl,
      PreCheckCompleted: "true",
      Username: username,
      Password: "Nx7!aQ2zR9kL",
      __RequestVerificationToken: token,
    },
    headers: {
      accept: "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
      origin: "https://login.xero.com",
      "sec-fetch-dest": "document",
      "sec-fetch-mode": "navigate",
      "sec-fetch-site": "same-origin",
      "upgrade-insecure-requests": "1",
    },
  });

  const body = (answer.body ?? "").toLowerCase();
  console.log("login:", {
    status: answer.status,
    refused: answer.refused,
    credential_error: body.includes("email address or password") || body.includes("incorrect"),
  });
} finally {
  await client.close();
}
```

`discover` reports the surface without running the sensor, so it is the cheapest way to tell whether a page is protected. `page` returns the document the session last loaded along with every input it declares, which saves a second fetch. `refused` is true on a 403, a 429, an access denied body or a challenge redirect, so a `false` there with a credential error in the body means the session passed and the login itself was rejected.

Events, deadlines, binary resolution and diagnostics work the same for every target and are covered on the [Node.js package page](/web-re-toolkit/packages/node/). What the client does and what the config controls is in [The Akamai client](/web-re-toolkit/guides/akamai/).
