---
title: Crates
description: What each of the toolkit's 24 crates contains, from crypto primitives to the code generator that emits language packages.
---

| Crate | Contents |
| --- | --- |
| `wre-core` | Errors, workspace paths, the artifact store, the capture bundle schema, the address grammar, hash primitives. |
| `wre-crypto` | XTEA, TEA, AES, RC4, XOR streams, pluggable block chaining including data dependent emission order, seeded pseudo-random generators, keyed substitution and permutations, murmur3, FNV, CRC32, repeating key recovery. |
| `wre-pack` | Custom alphabet base-N, variable radix streams with a shape fitter, linear digit encoding recovery, keyed digit rotation, charset membership bitfields. |
| `wre-pow` | Key derivations, hash chains, acceptance rules by prefix, leading zeros, folded modulus or score threshold, multi round challenges, parallel search. |
| `wre-ident` | Name blind shape hashing, weighted evidence for locating a role, cross build function pairing, the lock file and the drift report. |
| `wre-signals` | Value based slot alignment across builds, permutation and rotation recovery, noise filtering, provenance from an access trace. |
| `wre-sandbox` | A browser surface installed as native V8 bindings, a document, event and timer layer on top of it, a library of profiles captured off real devices, and a miss log. |
| `wre-oracle` | Finding the response feature that reflects payload state, and grading a built payload against real ones. |
| `wre-behavior` | Deterministic pointer, touch and key streams with timing that is not a constant. |
| `wre-net` | SOCKS5 proxies with session rotation, an HTTP client that emulates a browser's TLS and HTTP/2 fingerprint, ClientHello parsing and building, JA3 and JA4, HPACK with Huffman, the Akamai HTTP/2 fingerprint. |
| `wre-cdp` | Chrome lifecycle and reuse, a raw Chrome DevTools Protocol client over WebSocket, emulation profiles, Fetch-based script interception, a debugger with breakpoint-by-pattern and scope dumps. |
| `wre-probe` | Generates the in-page instrumentation script from a declarative surface spec. |
| `wre-capture` | Drives a run and writes a capture bundle. |
| `wre-js` | oxc-based parsing, a 26-pass deobfuscation pipeline run to fixpoint including control flow unflattening, evidence-based renaming, the surface index, self-integrity verify and re-sign, an equivalence gate, a byte-splice backend. |
| `wre-live` | An embedded V8 realm: mount a target, capture its functions as callable handles, host bridges, deterministic clock and random, execution timeouts. |
| `wre-env` | Captures a browser's object graph and materialises it lazily inside a realm. |
| `wre-vm` | Dispatch-loop discovery, concolic handler probing, an instruction intermediate representation, control flow recovery, a lifter to readable JavaScript. |
| `wre-wire` | Codecs, an addressable payload tree, diffing, forging, schema inference, round-trip verification. |
| `wre-variants` | One-fact-at-a-time sweeps, pooled group testing with a confirmation step, noise floor subtraction, signal attribution, 64 automation markers split into what a tool leaves behind and what hiding it leaves behind. |
| `wre-report` | Markdown tables, baseline diffing that ignores counter renames, the offline acceptance runner. |
| `wre-target` | The adapter manifest. |
| `wre-client` | The headless client software development kit: the `Client` trait, the op schema, the sidecar protocol, a rust consumer. |
| `wre-clientd` | `wred`, the host process that runs compiled clients and answers the protocol. |
| `wre-codegen` | Turns a bundle descriptor into typed node, python, go and rust packages. |
| `wre-cli` | The `wre` binary. |
