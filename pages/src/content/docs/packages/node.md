---
title: Node.js
description: Install a generated client package or the runtime, open a session, and call a target's ops from Node.js.
---

## Install a client

```bash
npm install @proofofbot/client-akamai
```

The `wred` binary for your platform arrives as an optional dependency. Node 18 or later.

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

## Install the runtime

Use the runtime to drive a binary that has no generated package, or several targets from one process.

```bash
npm install @proofofbot/runtime
```

```js
import { connect, WreError, ErrorKind } from "@proofofbot/runtime";

const sidecar = await connect({ binary: "/path/to/wred" });
console.log(sidecar.hello.schema_hash);

const session = await sidecar.open("example", {});

try {
  const result = await session.call("solve", { url: "https://acme.example/" }, {
    deadlineMs: 20000,
  });
  console.log(result);
} catch (err) {
  if (err instanceof WreError && err.kind === ErrorKind.Blocked) {
    console.error("target reported a block, retryable:", err.retryable);
  }
  throw err;
}

await session.close();
await sidecar.shutdown();
```

One sidecar process serves many sessions. A session owns the expensive state, a mounted realm or a cookie jar, so open one and reuse it.

## Events

A session is not an event emitter. Events are correlated by call id and delivered to a callback, either for the whole connection or for one call:

```js
const client = await AkamaiClient.open({ page_url: "https://acme.example/" }, {
  onEvent: (id, event, data) => console.log(event, data),
});

await client.solve({}, {
  onEvent: (id, event, data) => console.log(event, data),
});
```

Both fire when both are set: the per-call handler first, then the connection handler. A throw inside either is swallowed.

## Deadlines and cancellation

`deadlineMs` travels on the wire, so the sidecar stops the work rather than the caller abandoning a promise. `signal` takes an `AbortSignal` and sends a cancel frame.

```js
const abort = new AbortController();
setTimeout(() => abort.abort(), 5000);

await client.solve({}, { deadlineMs: 180000, signal: abort.signal });
```

A deadline that passes rejects with `kind === "timeout"`, an abort with `kind === "cancelled"`.

## Binary resolution

`connect()` uses the `binary` option if given, otherwise falls back to `resolveBinary()`, which checks `WRE_BINARY` first. Set `WRE_BINARY` to an absolute path to skip the package's shipped or downloaded binary. That path is used as-is, with no hash check.

## Errors

Every rejection from the sidecar is a `WreError` with a stable `kind`. Branch on `kind`, not on `message`. The [kind table](/web-re-toolkit/packages/#error-kinds) lists all nine.

## Sidecar output and diagnostics

The sidecar writes its log to its own stderr, which is discarded by default so a library does not print into a host process. Pass `{ stderr: "inherit" }` to see it, or set `WRE_STDERR=inherit`.

A failed call writes a report and puts its path in `error.detail.diagnostics`. `WRE_DIAG=always` records every call, `WRE_DIAG=off` records none, and `await client.diagnose(true)` writes one on demand. Read one back with `wre client diag <file>`.
