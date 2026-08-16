---
title: Kasada from Node.js
description: Install @proofofbot/client-kasada, answer an interrogation, and fetch the page again through the same session.
---

```bash
npm install @proofofbot/client-kasada
```

Node 18 or later. The `wred` binary for your platform arrives as an optional dependency.

A Kasada session mounts a graph profile, so capture one before the first run:

```bash
wre sandbox capture --graph --open --label "this machine"
```

```js
import { KasadaClient } from "@proofofbot/client-kasada";

const client = await KasadaClient.open({ page_url: "https://acme.example/buy" });

const solved = await client.solve({}, { deadlineMs: 120000 });
console.log(solved.verdict, solved.clearance);

const page = await client.request({ url: "https://acme.example/buy" });
console.log(page.status, page.bytes);

await client.close();
```

The token is bound to the `KP_UIDz` cookie the edge set on the interrogation, so solve against the url you actually want, then send everything else through the same client.

## A full run

Open one session, report what the page is serving, answer the interrogation, print how many of its own checks the agent flagged, then fetch the page again through the same session and list what came back. A session that never answered gets the interrogation instead of the page, which is the point of the comparison.

```js
import { open, WreError } from "@proofofbot/client-kasada";

const PAGE = process.env.PAGE ?? "https://www.realestate.com.au/buy/in-sydney,+nsw/list-1";

const listings = (html) => {
  const found = new Set();
  const pattern = /href="(\/property-[^"]+)"/g;
  let match;

  while ((match = pattern.exec(html)) !== null) found.add(match[1]);

  return [...found];
};

const client = await open({ page_url: PAGE });

try {
  const surface = await client.discover({});
  console.log(`${PAGE} answered ${surface.status}, protected ${surface.protected}`);

  if (!surface.protected) {
    console.log("no interrogation is being served, nothing to solve");
  } else {
    const solved = await client.solve({}, { deadlineMs: 120000 });
    console.log(`verdict ${solved.verdict}, clearance ${solved.clearance}`);
    console.log(`payload ${solved.payload_bytes} bytes in ${solved.ms} ms`);

    const report = await client.report();
    console.log(`the agent flagged ${report.flagged.length} of its own checks`);
  }

  const page = await client.request({ url: PAGE }, { deadlineMs: 60000 });
  console.log(`page ${page.status}, ${page.bytes} bytes`);

  const found = listings(page.body);
  console.log(`${found.length} listings`);
  for (const listing of found.slice(0, 10)) console.log(`  https://www.realestate.com.au${listing}`);
} catch (error) {
  if (error instanceof WreError) {
    console.error(`${error.kind}: ${error.message}`);
    process.exitCode = 1;
  } else {
    throw error;
  }
} finally {
  await client.close();
}
```

`examples/node/kasada-scrape.mjs` in the repository is the same script written against `@proofofbot/runtime` instead of the generated client, which is what you use when you drive several targets from one process.

Events, deadlines, binary resolution and diagnostics work the same for every target and are covered on the [Node.js package page](/web-re-toolkit/packages/node/). What the client does and what the config controls is in [The Kasada client](/web-re-toolkit/guides/kasada/).
