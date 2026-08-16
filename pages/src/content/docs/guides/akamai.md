---
title: The Akamai client
description: Warm an Akamai Bot Manager session without a browser and carry its cookies into your own requests.
---

`clients/akamai` warms an Akamai Bot Manager session without a browser and then carries it: it fetches the page, finds the sensor script the page names, runs that script unmodified inside a `wre-live` realm with the `wre-sandbox` browser surface, posts what the script builds, and hands the session's cookies to whatever request you want to make next.

Nothing about the payload is reimplemented. The vendor's own script computes it, which is why a rebuild of the sensor does not break this client.

```
wre client describe akamai
```

## The flow

`solve` does the whole warm-up:

1. `GET` the page with navigation headers, keeping cookies in a per-session jar.
2. Read the surface out of the HTML: the sensor script, the pixel client, the `bazadebezolkohpepadr` seed and the config bits the script's own URL carries.
3. `GET` the sensor script with script headers and the page as referer.
4. Mount the page in the sandbox: the profile's device values, the page's own HTML with its forms and inputs, `document.cookie` wired to the jar, requests wired to the host.
5. Run the script, fire `readystatechange`, `DOMContentLoaded` and `load`, play a pointer, click and key stream, then let the clock run.
6. Read `bmak.get_telemetry()`, post the payload it carries to the collection endpoint, and repeat for as many rounds as asked.

After that, `request` sends anything you like with the session's cookies, and `--telemetry` attaches a fresh `akamai-bm-telemetry` header built from the same realm.

```json
{ "op": "solve", "params": { "url": "https://login.example.com/" } }
{ "op": "request", "params": { "url": "https://api.example.com/orders", "telemetry": true } }
```

The other ops: `discover` reports the surface without running anything, `page` hands back the page HTML and its form fields, `payload` builds a fresh payload from the open session, `post` posts one, `cookies` reads the jar, `pow` answers a proof-of-work challenge, `pixel` runs the pixel client, `reset` drops the sandbox.

## What the config controls

| field | default | what it does |
| --- | --- | --- |
| `page_url` | none | page the session warms against when an op does not name one |
| `profile` | first in the library | sandbox profile id from `wre sandbox list` |
| `random_profile` | `false` | pick a captured profile at random instead |
| `fingerprint` | from the user agent | transport fingerprint as `profile[:platform]` |
| `user_agent` | the profile's | overrides what the sandbox and the transport both claim |
| `wait_ms` | `4000` | how long the sensor runs after load |
| `paced` | `true` | spend that wait in real time, so the payload's clock matches the edge's |
| `behaviour` | `true` | play a pointer, click and key stream into the page |
| `pixel` | `true` | run the pixel client when the page serves one |
| `live_xhr` | `false` | let the sensor's own requests leave the sandbox |
| `rounds` | `2` | payloads posted per solve |
| `init_cost_ms` | `25` | clock charge applied when the script writes `bmak.startTs` |

`live_xhr` is the one worth understanding. With it off, the sandbox answers the script's own `XMLHttpRequest` with `201 {"success":true}` and the host posts the payload the script built, once per round, with the headers a browser sends. With it on, every repost the script schedules goes out for real, which on a twenty second dwell is five or six posts in a couple of seconds of wall time. Measured against Xero's login endpoint on 2026-08-16, that is the difference between being served and being refused: same payload, same profile, same transport.

## The device profile

The sandbox profile is where the payload's device fields come from, so a captured profile beats the built-in one. `wre sandbox capture --open` drives a real Chrome and stores what it reads; the client picks the first profile in `profiles/` unless told otherwise, and falls back to the built-in when the library is empty.

Keep the transport and the profile in step. The client sends the profile's user agent and lets `wre-net` pick the closest emulation profile for it. If the captured Chrome is newer than any profile `wreq` carries, the handshake lands on the newest one it has while the header says what the device said.

## What was measured

Against `https://login.xero.com/identity/user/login`, which refuses a request whose session never posted a believable payload and answers a credential error when it accepts one:

| arm | login answer |
| --- | --- |
| no sensor run at all | 403, Akamai access denied |
| the word `garbage` posted as the payload | 403 |
| what this client builds | 200, the credential error page |

`clients/akamai/tests/gate.rs` is those three arms; it reaches the live endpoint and is `#[ignore]`d by default.

```
cargo test -p wre-client-akamai --test gate -- --ignored --test-threads=1
```

`_abck` stays at field 2 `-1` through all of it, including the arm that passes, so whatever that endpoint reads it is not the cookie's validation state. Every payload post answers `201` regardless of what is in it, so a `201` there means nothing on its own.

The offline suite in `clients/akamai/tests/session.rs` runs the same client against a local edge that serves its own sensor script, and covers discovery, the sandbox run, the posts, the pixel client, the cookie jar and a protected endpoint that refuses a session it did not warm.

## Proof of work

The challenge arrives in `_abck` field 4 as `<id>-<salt>-<difficulty>-<delay>-<slice>[-<version>]`, several separated by `||`. The `pow` op parses them, takes the first (preferring version 2), and searches ten rounds: round `n` uses `difficulty + n` as the modulus and looks for a nonce where `sha256(token + startTs + salt + modulus + nonce)`, read as a big-endian integer, is zero modulo that modulus. The answer is formatted as the four `;`-separated lists the script publishes.

```json
{ "op": "pow", "params": { "abck": "<cookie>", "start_ts": 1760000000000 } }
```

No host we have looked at asks for one: field 4 is `-1` everywhere, and the config bit that enables the client-side solver is off. The search and the answer format are checked against the format the script parses and against themselves, never against a live challenge.

## The pixel challenge

When the page carries a `bazadebezolkohpepadr` seed and an `/akam/<gen>/<hash>` script alongside an obfuscated sensor, that script is the pixel client. The client seeds the realm, runs it in the same sandbox, and replays the form-encoded POST it produces to `pixel_<hash>`, where the hash is `(77 ^ seed).toString(16)`. As with the sensor, nothing about that payload is rebuilt here.

## What is not handled

`sec_cpt` is parsed, not solved. The cookie, the challenge state in `_abck` and a challenge interstitial in the page are all reported in `solve`'s `challenge` field, and that is where it stops: no host in the research set ever served one, so there is nothing to write an answer against that would not be a guess.

The built-in profile has no speech synthesis voices and no audio render hash, so a run on it records those two misses. A captured profile fills them.
