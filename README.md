# web-re-toolkit

A Rust toolkit for reverse engineering client-side web protections. Record a browser run, deobfuscate the shipped script, call the script's own primitives, lift its virtual machine to readable JavaScript, and attribute wire fields to the environment facts that move them.

Everything target-specific lives in one manifest. Everything else is shared library code.

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

## Crates

| crate | contents |
| --- | --- |
| `wre-core` | errors, workspace paths, the artifact store, the capture bundle schema, the address grammar, hash primitives |
| `wre-net` | SOCKS5 proxies with session rotation, an HTTP client, ClientHello parsing and building, JA3 and JA4, HPACK with Huffman, the Akamai HTTP/2 fingerprint |
| `wre-cdp` | Chrome lifecycle and reuse, a raw CDP client over WebSocket, emulation profiles, Fetch-based script interception, a debugger with breakpoint-by-pattern and scope dumps |
| `wre-probe` | generates the in-page instrumentation script from a declarative surface spec |
| `wre-capture` | drives a run and writes a capture bundle |
| `wre-js` | oxc-based parsing, a 25-pass deobfuscation pipeline run to fixpoint, evidence-based renaming, the surface index, a byte-splice backend |
| `wre-live` | an embedded V8 realm: mount a target, capture its functions as callable handles, host bridges, deterministic clock and random, execution timeouts |
| `wre-env` | captures a browser's object graph and materialises it lazily inside a realm |
| `wre-vm` | dispatch-loop discovery, concolic handler probing, an instruction IR, control flow recovery, a lifter to readable JavaScript |
| `wre-wire` | codecs, an addressable payload tree, diffing, forging, schema inference, round-trip verification |
| `wre-variants` | one-fact-at-a-time sweeps, noise floor subtraction, signal attribution, a catalogue of automation markers |
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

[docs/CLIENTS.md](docs/CLIENTS.md) is the authoring guide. [docs/PROTOCOL.md](docs/PROTOCOL.md) is the wire contract.

## Targets

| target | adapter | client | research |
| --- | --- | --- | --- |
| altcha | `targets/altcha.toml` | `clients/altcha` | [docs/research/altcha.md](docs/research/altcha.md) |
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

**Snapshot the browser instead of writing DOM stubs.** `wre env snapshot` walks the real object graph into JSON. `wre env run` rebuilds it lazily inside a realm. Surfaces that cannot be faked in a headless realm route to a host bridge or a replay table.

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

`wre-net` computes and builds transport fingerprints and can shape a ClientHello for measurement. Impersonating a specific JA3 across a live TLS session needs a custom TLS stack and is not included.

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
