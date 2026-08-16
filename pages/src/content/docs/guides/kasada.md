---
title: The Kasada client
description: Answer a Kasada interrogation without a browser and carry the token the edge issues into your own requests.
---

`clients/kasada` answers a Kasada Bot Defence interrogation without a browser. It fetches the url you want, and when the edge answers with the interrogation page instead, it runs that page's own `ips.js` in a `wre-live` realm, lets the script post what it builds, and keeps the token the edge answers with.

Nothing about the payload is reimplemented. The vendor's own script computes it, which is why a rebuild of the agent does not break this client.

```
wre client describe kasada
```

## The flow

`solve` is the whole thing:

1. `GET` the url with navigation headers, keeping cookies in a per-session jar.
2. If the answer carries an interrogation, read the tenant path and the `ips.js` url out of it.
3. `GET` the agent script with script headers and the page as referer.
4. Mount the graph profile in the sandbox, at the url the page was served from, with a pool of child realms ready for the iframes the script makes.
5. Run the page's inline preamble, then the agent, under its own url as the script origin.
6. Pump timers and answer the script's requests until the script posts to `/tl` and the edge answers.

What comes back is the token, the clearance, the size of the payload the script built, every request the sandbox made, and the jar.

```json
{ "op": "solve", "params": { "url": "https://www.example.com/buy" } }
{ "op": "request", "params": { "url": "https://www.example.com/buy" } }
```

The token is bound to the `KP_UIDz` cookie the edge set on the interrogation, so solve against the url you actually want. `request` then sends your request through the same jar, transport fingerprint and user agent, carrying the token as headers and as that cookie.

The other ops: `discover` reports the wiring without running anything, `loader` mounts the site's own `p.js`, `pow` builds a proof of work header, `payload` hands back the sealed body, `vector` hands back the signal array behind it, `report` decodes what the agent said about itself, `cookies` reads the jar, `misses` lists what the sandbox could not answer, `reset` drops the realm.

## What the config controls

| field | default | what it does |
| --- | --- | --- |
| `page_url` | none | url the session solves for when an op does not name one |
| `profile` | first captured, else the bundled one | graph profile id, from `wre sandbox list` |
| `fingerprint` | from the user agent | transport fingerprint as `profile[:platform]` |
| `user_agent` | the profile's | overrides what the sandbox and the transport both claim |
| `proxy` | none | http or socks5 url the session and the sandbox both go through |
| `wait_ms` | `20000` | how long to let the agent run before giving up on a token |
| `step_ms` | `100` | how often the timer queue is drained |
| `paced` | `true` | spend the wait in real time, so the payload's clock matches the edge's |
| `frames` | `4` | child realms opened up front for the iframes the agent creates |
| `report` | `false` | let the agent's self report reach `reporting.cdndex.io` |
| `capture_vector` | `false` | keep the signal array the agent built, for the `vector` op |
| `version` | `j-1.2.661` | build version to claim when the page names none |

`frames` is worth understanding. The agent creates iframes and reads the document's own functions back through the frame's `Function.prototype.toString`, which is how it checks whether anything has been patched. Each frame is a real second V8 context in the same isolate, installed from the same graph, sharing one table of function sources with the document. A run that needs more frames than the pool holds records a miss and the agent gets `null` where a browser gives it a window.

## The graph profile

The interrogation enumerates the whole global surface: every own property of `window`, in order, with shapes and sources. A property table cannot answer that, so this client mounts a **graph profile**, which is a captured object graph rather than a list of readings.

One graph is compiled into the binary, `macos-chrome-151`, captured from a real MacBook running Chrome 151. A session with nothing captured mounts that, so `solve` works on a fresh install and with no workspace on disk.

Capture your own when you want a graph that is not shared with every other user of this toolkit, or one from a different browser or platform:

```
wre sandbox capture --graph --open --label "MacBook Pro, Chrome 151"
wre sandbox list
```

That writes `profiles/graph/<id>.json`. `wre sandbox list` marks each graph `captured` or `bundled`.

Selection: `profile` names an id, from the captured directory first and the bundled set second. With no `profile`, the first captured graph is used, and the bundled one when nothing is captured.

## Contributing a profile

A graph is per browser build and per platform, so the bundled set only covers what people have sent in. To add yours:

1. `wre sandbox capture --graph --open --label "ThinkPad X1, Chrome 152 on Linux"`
2. `gzip -9 -c profiles/graph/<id>.json > crates/wre-sandbox/assets/graph/profiles/<id>.json.gz`
3. Open a PR with that one file. The build script picks up every `.json.gz` in that directory, so nothing else needs editing.

Use an id that names the browser and platform, `linux-chrome-152`, not the machine. A gzipped graph is around 600 KB. The capture is a real reading of your machine, including user agent, system colors, audio and WebGL values, and it becomes public in the repository, so send one from a machine you are happy to describe. Changing which id is the default is a separate change to `BUNDLED_ID` in `crates/wre-sandbox/src/graph.rs`.

[The browser surface](/web-re-toolkit/guides/sandbox/) covers what a graph carries and how it differs from the property profiles the other clients use.

Two of the payload's fields are the message V8 builds for `class X extends <value> {}`, which embeds the value's internal function name and whether it is native. Neither can be set on a JavaScript function: redefining `name` does not reach the message, and a body of `{ [native code] }` does not parse. `document.createElement` and `Permissions.prototype.query` are therefore rebuilt as real V8 functions that forward to the environment's own implementations, so the engine reports the names Chrome reports.

## The proof of work

Sites that enable it want an `x-kpsdk-cd` header on every stamped request. It is cleartext JSON: a nonce, the client's clock, its estimate of the server's, and the answers to a short sha256 chain seeded from the token and a salt baked into the loader.

Two ways to get one.

```json
{ "op": "pow", "params": { "salt": "<64 hex characters from p.js>" } }
```

computes it in Rust from the solved token. Or mount the site's own loader and let it stamp:

```json
{ "op": "loader", "params": {} }
{ "op": "request", "params": { "url": "https://www.example.com/api/search", "stamped": true } }
```

A stamped request is sent from inside the realm, so the loader builds every `x-kpsdk-*` header itself, proof of work included, and a rebuild of the loader cannot drift from it.

## Reading a run

The agent writes a report about itself and posts it to `reporting.cdndex.io` when the run is sampled. Every check it ran is in there by name with what it found, XORed with a nine byte key. The client holds that post back by default, decodes it, and hands it to you:

```json
{ "op": "report", "params": {} }
```

```json
{
  "posted": 1,
  "about": { "build": "…", "version": "j-1.2.661", "checks": 123 },
  "flagged": []
}
```

An empty `flagged` is the goal: it means the agent found nothing to say about the environment it ran in. A flagged check names itself and quotes the value it objected to, which is a better oracle than the payload, because the payload only says a field disagrees.

`vector` is the other half. With `capture_vector` on, the client keeps the signal array the script built before it sealed it, so you can diff it against a browser's for the same build.

## What was measured

Against `https://www.realestate.com.au/buy`, which answers `429` and an interrogation to anything that has not solved one:

| arm | answer |
| --- | --- |
| no session at all | 429, the interrogation |
| a session that solved the interrogation | 200, the listings page |

`clients/kasada/tests/gate.rs` is those arms plus a check that the agent's own report is empty. It reaches the live endpoint and is `#[ignore]`d by default.

```
cargo test -p wre-client-kasada --test gate -- --ignored --test-threads=1
```

The offline suite in `clients/kasada/tests/session.rs` runs against a local edge that serves its own interrogation, and covers discovery, the token headers, the jar, the proof of work and the ops that refuse to run without a session.

## What is not handled

The `graphics` table, which answers the canvas and WebGL readbacks, is a recorded reply list rather than a renderer. `wre sandbox capture --graph --calls <file>` replays a call list on the capturing machine and stores the answers. Without it, canvas readbacks answer blank and the run records misses.

The clock is real. `paced` off makes a run finish sooner, and the durations the payload carries then read as a machine rather than a person.
