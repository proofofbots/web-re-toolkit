# ALTCHA

ALTCHA is an open source captcha widget (MIT) that replaces the "click the traffic lights" interaction with a proof of work the browser computes. The server issues a signed challenge, the widget brute forces a counter until a derived key starts with a published prefix, and the form carries the answer as a base64 JSON field. This note describes v3.2.1 of `altcha-org/altcha`, the format it still accepts from v1 deployments, and what all of that means for a headless client.

Everything here is read from the published source. Nothing is obfuscated: the shipped bundle is minified ESM built from TypeScript, and the same code is on npm. The interesting work is not recovering the algorithm, it is knowing exactly which bytes go into each hash and where the cost actually lands.

## The pieces

- `src/pow.ts` creates, solves, signs and verifies challenges.
- `src/algorithms/{sha,pbkdf2,scrypt,argon2id}.ts` are the four key derivation functions, registered by name in `src/entry.ts` as `SHA-256`, `SHA-384`, `SHA-512`, `PBKDF2/SHA-256`, `PBKDF2/SHA-384`, `PBKDF2/SHA-512`, `ARGON2ID` and `SCRYPT`.
- `src/workers/*.ts` wrap the same solver in Web Workers, one per algorithm.
- `src/his.ts` is the interaction collector, the only part that looks at the user rather than the CPU.
- `src/plugins/obfuscation.plugin.ts` hides contact details behind a challenge.
- `src/server-signature.ts` checks the signed verdict that the hosted service (Sentinel) returns.

## The v3 protocol

A challenge is JSON:

```json
{
  "parameters": {
    "algorithm": "SHA-256",
    "cost": 10,
    "keyLength": 32,
    "keyPrefix": "a9a9e81c084ad44c25351c5f5568ce22",
    "nonce": "9295b841727d48460ca4b4954584a14f",
    "salt": "b83df484bc74cdde5caad7992284ca01"
  },
  "signature": "b3aeb49b8a27accf753a38d53e180ce8aaf6cacf72480719f4872d884574a168"
}
```

The solver walks a counter from 0 upward. For each candidate it builds a password buffer, the nonce bytes followed by the counter as a big endian uint32, derives a key from `(salt, password)` with the named function, and compares. `keyPrefix` is compared as bytes when its length is even and as a hex string prefix otherwise, so the default 16 byte prefix means one candidate in 2^128 matches by chance: the counter the server used is the only answer a solver will find.

The payload posted with the form is `btoa(JSON.stringify({ challenge: { parameters, signature }, solution: { counter, derivedKey, time } }))`. The server re-derives the key from the counter and compares, or takes the faster path and checks `keySignature`, an HMAC over the derived key that only it can produce.

`expiresAt` is a unix second inside `parameters`, which means it is covered by the signature.

### The four derivations

`SHA-*` is a hash chain, not a single hash. The first input is `salt || password`; each further iteration hashes the previous output, and every output is truncated to `keyLength` before it is fed back:

```ts
data = i === 0 ? concat(salt, password) : derivedKey;
derivedKey = digest(algorithm, data).slice(0, keyLength);
```

`cost` is the iteration count, floored at 1.

`PBKDF2/*` calls `crypto.subtle.deriveKey` with `iterations = cost` and asks for an AES-GCM key of `keyLength * 8` bits. That last detail is a real constraint, not a formality: WebCrypto only mints AES keys of 128, 192 or 256 bits, so a server that issues a PBKDF2 challenge with `keyLength: 48` gets `OperationError: AES key length must be 128, 192, or 256 bits` in every browser. Only 16, 24 and 32 are usable.

`SCRYPT` maps `cost` to N, `memoryCost` to the block size r (default 8) and `parallelism` to p (default 1). N is a cost factor here, so it has to be a power of two. `ARGON2ID` maps `cost` to iterations, `memoryCost` to KiB of memory (default 16384) and `parallelism` to lanes. Both run through `hash-wasm`.

### The signature

`signature` is `HMAC(secret, canonicalJSON(parameters))`, hex, SHA-256 unless the server picked another HMAC digest. Canonical here means `JSON.stringify` over an object whose keys were sorted recursively, with `undefined` values dropped; arrays are left in their original order and objects nested inside arrays are not sorted, because `sortKeys` returns arrays untouched. Reproducing the exact string is the whole game when you want to check a challenge before spending CPU on it.

Verification order on the server is: expiry, signature present, signature valid, then the solution. A tampered `keyPrefix` or `cost` is caught by the signature check before anything is derived.

## The v1 format

Deployments running the older server library send a flat object, and v3 widgets still accept it:

```json
{
  "algorithm": "SHA-256",
  "challenge": "f4cb24d4a5bffabadd37c4ce3cfb5246adc8f2cb88e3e49497f0b3c3af90ffaf",
  "salt": "saltysalt?expires=1700000000",
  "signature": "c053bcd07083810a8a9412efb35168a56b6a2d8ca03bbe9e52ceceb69f591d9a"
}
```

`Widget.svelte` rewrites it into the v3 shape: `nonce` becomes the hex of the salt string's UTF-8 bytes, `salt` becomes empty, `cost` becomes 1, `keyPrefix` becomes the full challenge hash, and the counter mode switches from `uint32` to `string`. Run those substitutions through the SHA-256 chain and what is left is the original v1 rule, `sha256(salt + number) === challenge`, with the counter appended as decimal text.

Two details survive only in the legacy path. The expiry lives in a query string glued to the salt (`?expires=`), so it is signature protected only because the salt is hashed into the challenge. And the payload keeps the old flat shape, `{ algorithm, challenge, number, salt, signature, took }`, with the original salt string including its query, not the hex nonce. The v1 signature is an HMAC over the challenge hash string, not over any JSON.

## Where the cost lands

Measured on an Apple M3 (8 cores), release build, one candidate = one derivation:

| algorithm | rust, 1 thread | rust, 8 threads | node WebCrypto, 1 thread |
| --- | --- | --- | --- |
| `SHA-256`, cost 1 | 7,300,000/s | 33,000,000/s | 106,000/s |
| `SHA-256`, cost 100000 | 177/s | 740/s | 1/s |
| `PBKDF2/SHA-256`, cost 5000 | 2,800/s | 11,200/s | 1,700/s |

The PBKDF2 row is the honest one: WebCrypto runs the iteration loop in native code, so a native solver gains about 1.6x per thread and the rest of its advantage comes from using every core instead of the widget's capped 16 workers.

The SHA rows are where the design leaks. `deriveKey` awaits `crypto.subtle.digest` once per iteration, so a `cost` of 100000 costs the browser 100000 promise round trips per candidate, and the widget manages roughly one candidate per second while a native solver does 177 on one thread. Raising `cost` on a SHA challenge raises the defender's cost against real users far faster than it raises an attacker's. A PoW captcha's premise is a bounded, roughly symmetric cost; the SHA path with a high cost is neither, and the parameters worth deploying are the ones where the work happens inside one native call.

None of this breaks anything. A solved challenge is a solved challenge, and the widget's own solver produces the same answer, only slower. What limits automated submissions is on the server: challenge expiry, single use enforcement, rate limits per issuer, and, if the operator pays for it, Sentinel's scoring.

## Human interaction signature

Newer server flows answer the challenge request with `{ "his": { "url": "..." } }` instead of a challenge. The widget then POSTs `{ his: collector.export() }` to that URL and gets the challenge back. The collector (`src/his.ts`) listens with capture and passive listeners for `focusin`, `keydown`, `pointerdown`, `pointermove`, `scroll` and `touchmove`, and keeps at most 60 samples per buffer at a 50 ms floor:

```json
{
  "focus": [[elapsedMs, tabIndex, tagCode, hadInteraction]],
  "maxTouchPoints": 0,
  "pointer": [[x, y, t]],
  "scroll": [[y, t]],
  "time": 1700000000000,
  "touch": [[x, y, t, force, radiusX, radiusY]]
}
```

`tagCode` is a small enum over `INPUT`, `TEXTAREA`, `SELECT`, `BUTTON`, `A`, `DETAILS`, `SUMMARY`, `IFRAME`, `VIDEO`, `AUDIO`; `hadInteraction` is 1 when a key or pointer event fired within 100 ms before the focus. Touch samples are only recorded for `pointerType === 'touch'` movement, and pointer samples are skipped for it, so a payload with both populated did not come from one device.

What the server does with this is not in the repo, so the scoring side is unknown. The shape it accepts is not: coordinates are integers, timestamps are event timestamps in milliseconds since page load, and the arrays are capped. Our client synthesises samples that satisfy those constraints from a seeded generator (a quadratic Bezier toward the widget with jitter and eased spacing). That produces well formed input, not a replay of measured human motion, and it will not defeat a classifier trained on real traces.

## The obfuscation plugin

`data-obfuscated` on the widget holds base64 JSON of a PBKDF2 challenge plus `cipher: { iv, data }`. Solving the challenge yields the derived key, and the derived key *is* the AES-GCM key over the hidden text, which is usually a `mailto:` or `tel:` link. The published `keyPrefix` is the first half of the key (32 hex characters of a 32 byte key), so it identifies the answer without revealing it. Defaults are `PBKDF2/SHA-256`, cost 5000, counter between 20 and 200, which is a second or so in a browser and a few milliseconds natively. As an email harvesting deterrent it costs a scraper about 200 PBKDF2 derivations per address.

## Server signature

Sentinel returns a payload whose `verificationData` is a URL encoded query string (`verified`, `score`, `classification`, `expire`, `fields`, `fieldsHash`, ...). The signature is `HMAC(secret, SHA-256(verificationData))`, an HMAC over the digest bytes rather than over the string. `fieldsHash` binds named form fields to the verdict: the values are joined with newlines in the order `fields` lists them and hashed. Verification treats `verified !== true` in either the payload or the parsed data as an invalid solution.

## The playground

`playground.altcha.org` is a static SPA. It creates challenges in the page with the hard coded secret `signature.secret` and verifies them locally, so it exercises the widget and the algorithm switch, not a server. Useful for reading behaviour, useless as a live target.

## What is in this repo

- `targets/altcha.toml` is the adapter: discovery patterns for the widget script and its `challengeurl` attribute, mount signatures keyed on the string literals that survive minification, `base64-json` wire codec with field labels, probe surfaces for `SubtleCrypto` and the interaction events, and knobs for core count, touch emulation and the automation flag.
- `clients/altcha` is the headless client. It ports the four derivations to Rust rather than mounting the widget in V8, because the shipped code is an unobfuscated wrapper around WebCrypto and hash-wasm, and because a mounted realm has no WebCrypto to wrap.
- `conformance/altcha.json` runs 13 cases through the sidecar, including both challenge formats, the obfuscation round trip and the server signature.
- `reference/altcha/vectors.json` holds the values the widget's own code produced, and `reference/altcha/vectors.test.ts` regenerates them: drop it into `tests/` of a clone of `altcha-org/altcha` and run `npx vitest run tests/vectors.test.ts`.

`clients/altcha/tests/session.rs` drives the client itself through create, solve and verify for both formats, and checks that a seeded session produces the same interaction samples twice. `clients/altcha/tests/vectors.rs` asserts the Rust port against the widget's own values: the SHA chain including truncation and a cost above 1, both PBKDF2 digests, scrypt, argon2id, the canonical JSON string and its HMAC, a full v3 solve that lands on the counter the server used, the v1 shape and signature, an obfuscation payload that decrypts to its cleartext, and a Sentinel signature with its `fieldsHash`.

## Limits

The client solves what a browser solves. It does not defeat rate limiting, challenge reuse detection, or a server that scores interaction data with a model. HIS synthesis produces structurally valid samples, and their statistical distance from real input is untested. Argon2id and scrypt challenges cost the same order of magnitude natively as in the browser, so a deployment that picks them and sets the parameters high gets what it paid for.
