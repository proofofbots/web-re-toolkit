<h1 align="center">web-re-toolkit</h1>

<h3 align="center">Akamai Bot Manager and Kasada Bot Defence solvers that run without a browser, and the reversing toolkit they were built with</h3>

<p align="center">
  <b>Akamai's V2 and V3 sensors and Kasada's interrogation, answered out of a small V8 sandbox instead of a browser.</b>
</p>

<p align="center">
  <b><a href="https://proofofbots.github.io/web-re-toolkit/packages/">Install the client</a></b> ·
  <b><a href="https://proofofbots.github.io/web-re-toolkit/guides/akamai/">How it works</a></b> ·
  <a href="https://proofofbots.github.io/web-re-toolkit/">Docs</a> ·
  <a href="https://discord.gg/nbBePnsa9">Discord</a>
</p>

---

| | |
| --- | --- |
| [**Akamai, solved headlessly**](https://proofofbots.github.io/web-re-toolkit/guides/akamai/) | V2 and V3 sensors, the pixel challenge, the proof of work, `_abck` and the cookie jar |
| [**Kasada, solved headlessly**](https://proofofbots.github.io/web-re-toolkit/guides/kasada/) | the interrogation, the `/tl` submission, the token the edge issues, and the page it opens |
| [**No browser**](https://proofofbots.github.io/web-re-toolkit/guides/sandbox/) | a V8 realm with a native browser surface, not Chrome, not Puppeteer, not a DOM shim |
| [**Nothing reimplemented**](https://proofofbots.github.io/web-re-toolkit/guides/akamai/) | the vendor's own sensor script computes the payload, so a rebuild does not break the client |
| [**Real devices, not generated values**](https://proofofbots.github.io/web-re-toolkit/guides/sandbox/) | profiles captured off actual browsers with `wre sandbox capture` |
| [**Browser transport**](https://proofofbots.github.io/web-re-toolkit/guides/clients/) | matching TLS and HTTP/2 fingerprints, header order, SOCKS5 with session rotation |
| [**node, python, go, rust**](https://proofofbots.github.io/web-re-toolkit/packages/) | one compiled binary, four generated packages, same protocol |

```js
import { AkamaiClient } from "@proofofbot/client-akamai";

const client = await AkamaiClient.open({ page_url: "https://login.example.com/" });

await client.solve({});
const answered = await client.request({ url: "https://api.example.com/orders", telemetry: true });

await client.close();
```

[Packages](https://proofofbots.github.io/web-re-toolkit/packages/) has the same run in python, go and rust.

## The toolkit

The client is one output of a Rust toolkit for reverse engineering client-side web protections. Record a browser run, deobfuscate the shipped script, call the script's own primitives, lift its virtual machine to readable JavaScript, and attribute wire fields to the environment facts that move them.

Everything target-specific lives in one manifest. Everything else is shared library code.

Documentation: https://proofofbots.github.io/web-re-toolkit/

Discord: https://discord.gg/nbBePnsa9

## Install

```bash
cargo build --release
./target/release/wre --help
```

The V8 build pulls a prebuilt static library on first compile, so the first build is slow and needs network access. Chrome is located automatically on macOS, Linux and Windows. Set `WRE_CHROME` to override.

## Project layout

```
my-research/
  wre.toml              marks the workspace root
  targets/acme.toml     the adapter, the only per-target file
  artifacts/            scratch, not committed
  captures/             pinned captures that survive an artifacts wipe
  reference/            generated tables and baselines
```

Create one with `wre init acme --url https://acme.example/`.

## A worked pass

```bash
wre discover https://acme.example/ --target acme
wre discover https://acme.example/ --target acme --fingerprint chrome_141:windows
wre capture --target acme --scripts
wre pin acme-2026-08-15
wre show captures/acme-2026-08-15

wre deobf artifacts/captures/.../collect.js --target acme --rename --stats
wre surface collect.clean.js

wre mount collect.js --target acme
wre mount collect.js --target acme --role seal --args '[{"a":1}]'

wre vm discover collect.js
wre vm probe collect.js --table HANDLERS --frame frame.js
wre vm lift program.json --out lifted.js

wre wire roundtrip body.bin --codec base64
wre wire diff before.json after.json
wre wire schema captures/*/payload.json

wre verify --target acme --capture captures/acme-2026-08-15
```

Finding the same code again after the vendor rebuilds:

```bash
wre locate collect.js --target acme --lock targets/acme.lock
wre drift targets/acme.lock collect-new.js
wre builds collect-old.js collect-new.js

wre integrity collect.js --target acme
wre integrity collect.patched.js --target acme --resign
wre equivalent collect.js collect.clean.js
```

Grading what you built, and planning the runs that attribute a detection:

```bash
wre grade built.json --real capture-1.json capture-2.json
wre align --before a1.json a2.json --after b1.json b2.json

wre sandbox capture --open
wre sandbox check --all
wre markers --kind concealment
wre pools
```

## Crates

| crate | contents |
| --- | --- |
| `wre-core` | errors, workspace paths, the artifact store, the capture bundle schema, the address grammar, hash primitives |
| `wre-crypto` | XTEA, TEA, AES, RC4, XOR streams, pluggable block chaining including data dependent emission order, seeded PRNGs, keyed substitution and permutations, murmur3, FNV, CRC32, repeating key recovery |
| `wre-pack` | custom alphabet base-N, variable radix streams with a shape fitter, linear digit encoding recovery, keyed digit rotation, charset membership bitfields |
| `wre-pow` | key derivations, hash chains, acceptance rules by prefix, leading zeros, folded modulus or score threshold, multi round challenges, parallel search |
| `wre-ident` | name blind shape hashing, weighted evidence for locating a role, cross build function pairing, the lock file and the drift report |
| `wre-signals` | value based slot alignment across builds, permutation and rotation recovery, noise filtering, provenance from an access trace |
| `wre-sandbox` | a browser surface installed as native V8 bindings, a document, event and timer layer on top of it, a graph mode that replays a captured object graph and its child realms, a library of profiles captured off real devices, and a miss log |
| `wre-oracle` | finding the response feature that reflects payload state, and grading a built payload against real ones |
| `wre-behavior` | deterministic pointer, touch and key streams with timing that is not a constant |
| `wre-net` | SOCKS5 proxies with session rotation, an HTTP client that emulates a browser's TLS and HTTP/2 fingerprint, ClientHello parsing and building, JA3 and JA4, HPACK with Huffman, the Akamai HTTP/2 fingerprint |
| `wre-cdp` | Chrome lifecycle and reuse, a raw CDP client over WebSocket, emulation profiles, Fetch-based script interception, a debugger with breakpoint-by-pattern and scope dumps |
| `wre-probe` | generates the in-page instrumentation script from a declarative surface spec |
| `wre-capture` | drives a run and writes a capture bundle |
| `wre-js` | oxc-based parsing, a 26-pass deobfuscation pipeline run to fixpoint including control flow unflattening, evidence-based renaming, the surface index, self-integrity verify and re-sign, an equivalence gate, a byte-splice backend |
| `wre-live` | an embedded V8 realm: mount a target, capture its functions as callable handles, host bridges, child contexts that pass objects between them, rebuilding a JavaScript method as a native one, deterministic clock and random, execution timeouts |
| `wre-env` | captures a browser's object graph and materialises it lazily inside a realm |
| `wre-vm` | dispatch-loop discovery, concolic handler probing, an instruction IR, control flow recovery, a lifter to readable JavaScript |
| `wre-wire` | codecs, an addressable payload tree, diffing, forging, schema inference, round-trip verification |
| `wre-variants` | one-fact-at-a-time sweeps, pooled group testing with a confirmation step, noise floor subtraction, signal attribution, 64 automation markers split into what a tool leaves behind and what hiding it leaves behind |
| `wre-report` | markdown tables, baseline diffing that ignores counter renames, the offline acceptance runner |
| `wre-target` | the adapter manifest |
| `wre-client` | the headless client SDK: the `Client` trait, the op schema, the sidecar protocol, a rust consumer |
| `wre-clientd` | `wred`, the host process that runs compiled clients and answers the protocol |
| `wre-codegen` | turns a bundle descriptor into typed node, python, go and rust packages |
| `wre-cli` | the `wre` binary |

## Headless clients

A client produces a valid payload for one service with no browser. Write it once in Rust under `clients/<id>`. The build turns it into node, python, go and rust packages that drive the same compiled binary over a pipe.

```bash
wre client new acme
wre client build --bundle default --sign
wre client package --bundle default
wre client test --lang all
```

A session owns the mounted realm and the cookies, so a caller opens one and reuses it. Each session records its calls and writes a single JSON report when a call fails. Read one back with `wre client diag <file>`. The report carries the mounted build tag, the script digest, the realm's console and error records, the call history, and the client's own `diagnostics` section, with credential-shaped values redacted.

[Headless clients](https://proofofbots.github.io/web-re-toolkit/guides/clients/) is the authoring guide. [The sidecar protocol](https://proofofbots.github.io/web-re-toolkit/reference/protocol/) is the wire contract.

The generated node, python and go packages are listed under [Packages](https://proofofbots.github.io/web-re-toolkit/packages/).

## Documents

Everything below lives on the [documentation site](https://proofofbots.github.io/web-re-toolkit/). The source is in `pages/src/content/docs`.

| document | what it covers |
| --- | --- |
| [Finding things again after a rebuild](https://proofofbots.github.io/web-re-toolkit/guides/identification/) | locating a target's roles without depending on the text of one build, and reading the next one |
| [The browser surface](https://proofofbots.github.io/web-re-toolkit/guides/sandbox/) | why it is native rather than JavaScript, and what it still does not have |
| [The Akamai client](https://proofofbots.github.io/web-re-toolkit/guides/akamai/) | running a live Akamai sensor headlessly and carrying the session it produces |
| [The Kasada client](https://proofofbots.github.io/web-re-toolkit/guides/kasada/) | answering a live Kasada interrogation headlessly and carrying the token it issues |
| [Headless clients](https://proofofbots.github.io/web-re-toolkit/guides/clients/) | writing a headless client |
| [The sidecar protocol](https://proofofbots.github.io/web-re-toolkit/reference/protocol/) | the sidecar wire contract |
| [Command reference](https://proofofbots.github.io/web-re-toolkit/reference/cli/) | every `wre` subcommand |

## Targets

| target | adapter | client | research |
| --- | --- | --- | --- |
| akamai | any protected page | `clients/akamai` | [akamai](https://proofofbots.github.io/web-re-toolkit/guides/akamai/) |
| kasada | any protected page | `clients/kasada` | [kasada](https://proofofbots.github.io/web-re-toolkit/guides/kasada/) |
| altcha | `targets/altcha.toml` | `clients/altcha` | [altcha](https://proofofbots.github.io/web-re-toolkit/research/altcha/) |
| example | `targets/example.toml` | `clients/example` | a worked adapter, not a real service |

## Core ideas

**Borrow, do not reimplement.** A target's crypto is already written and already correct. `wre-live` mounts the shipped script in a V8 realm and hands you its own functions as callable handles, so your decoder cannot drift from the build. Roles are matched by a regex against each top-level function's source, declared in the manifest.

```toml
[[live.signatures]]
role = "hash"
pattern = "0x811c9dc5|2166136261"
params = 1
```

**Concolic handler probing.** For a custom VM whose opcode handlers are real JavaScript functions, you do not need to read them. Run each handler against Proxy sentinels standing in for the registers and the operand reader, and record what it read, what it wrote, and whether it touched the program counter. Run it again with the first operand forced falsy and diff the two: a handler that behaves differently is a conditional branch. Only the frame model, meaning how the VM calls its handlers, is per-target, and it is about twenty lines of JavaScript.

**Handler identity beats opcode numbers.** Protections permute the opcode table per build. Keying a trace on which handler function ran, rather than on the opcode number, makes the permutation irrelevant and recovers the mapping between two builds (`wre vm align`).

**Snapshot the browser instead of writing DOM stubs.** `wre env snapshot` walks the real object graph into JSON. `wre env run` rebuilds it lazily inside a realm. `wre sandbox capture --graph` does the same walk for a profile the sandbox mounts directly, for targets that enumerate the surface rather than reading known fields. Surfaces that cannot be faked in a headless realm route to a host bridge or a replay table.

**Identity survives a rebuild.** Nothing important is found by matching the text of one build. A role is located by scoring several weak signals against a name blind normalisation of the AST: structural shape, magic constants, the property names it reaches, its position in the call graph, and where it matters, what it returns when you actually call it. The result is written to a lock file, and the next build is diffed against that lock, so a rebuild produces a report of what moved rather than a pattern that silently stops matching. [Finding things again after a rebuild](https://proofofbots.github.io/web-re-toolkit/guides/identification/) covers it.

**Native shapes, replayed values.** The sandbox installs its browser surface as real V8 bindings, so accessors are native functions and wrong receivers throw `Illegal invocation`. The document, timers and events sit on top of that, and the functions they add report as native through a V8-level `toString`, which is a narrower lie than a JavaScript patch over the whole surface. The values come from `profiles/`, one file per real device captured with `wre sandbox capture`; nothing is generated. [The browser surface](https://proofofbots.github.io/web-re-toolkit/guides/sandbox/) covers it.

**Attribute many facts in few runs.** Testing 64 automation markers one at a time is 64 page loads. A pooled design plants them in groups so that each marker sits in a unique combination of runs, which takes 7. A pooled verdict is then confirmed by planting that one marker alone, because two markers together can produce the pattern of a third.

**Subtract the noise floor.** Run the baseline twice before sweeping anything. Addresses that differ between two identical runs are noise, and every sweep result is reported with them removed.

**Every decode is checked by re-encoding.** `verify_roundtrip` opens a body, seals it again, and compares bytes. A codec that cannot reproduce the original is reported as such.

## What is per-target

Only the manifest, and everything in it is data:

- discovery patterns for finding the script and endpoints in a document
- primitive signatures and source patches for mounting
- the VM frame model and opcode labels
- the codec choice and field labels
- knob definitions for sweeps
- extra probe surfaces
- the check list for `wre verify`

Naming heuristics, coherence rules and opcode semantics are per-target data as well. They look like code and they are tables.

## Limitations

The deobfuscation passes produce a reconstruction, not a byte-equivalent program. Renaming and dead-binding removal change observable globals in a classic script, so `remove_unused` is off by default and on in the `readable` preset.

The lifter emits structured control flow when the CFG is reducible and falls back to a labelled dispatch loop when it is not. Both are correct. Unknown opcodes are lifted as `opN(args)` calls and reported, not guessed.

`wre-net` sends requests through `wreq`, so the ClientHello, HTTP/2 settings and header order match the browser profile the client emulates, and a client built from a user agent picks the nearest profile for that agent so the transport and the header agree. The profile set is whatever `wreq-util` ships, so a browser release is only reachable once it lands upstream, and a profile is one build's snapshot: it carries no per-installation variation, and nothing here hides a mismatch between the emulated client and the payload it sends.

The property profile the sensor clients mount has no layout. A target that creates an element and measures it gets zero. A graph profile carries a measured layout table and answers those, within the set of elements the capture measured; anything outside it records a miss.

Similarity between two builds is a structural estimate, not a proof. A function reported as edited at 90 percent shared structure still has to be read.

The V8 realm is a real engine with a fake environment. A target that reaches for something the snapshot did not capture gets `undefined`, which appears in the probe records.

## Releasing

Tag a version. `.github/workflows/release.yml` cross builds `wred` for five platforms, generates the packages, runs the conformance suite against the release binary, attaches the binaries to the GitHub release, and publishes to npm and PyPI over OpenID Connect.

```bash
bash scripts/set-version.sh 0.2.0
git commit -am "chore: release 0.2.0"
git tag v0.2.0 && git push origin main --tags
```

## Testing

```bash
cargo test --workspace
bash scripts/smoke.sh
bash scripts/client-smoke.sh
```

`smoke.sh` exercises the CLI end to end: deobfuscating an obfuscated sample, mounting a target and calling its hash function, discovering and probing a toy VM, lifting an instruction stream, round-tripping and diffing payloads, replaying an environment snapshot, and computing an HTTP/2 fingerprint. `client-smoke.sh` builds `wred`, runs the conformance suite through all four bindings, generates the packages, and calls the generated node, python and go packages against the built binary.
