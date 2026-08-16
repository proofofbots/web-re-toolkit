---
title: Packages
description: The published npm, PyPI, Go and Rust packages for every headless client, and the runtime each language binding depends on.
---

Every client is written once in Rust and compiled into one host binary, `wred`. The language packages are generated from that binary's descriptor and drive it over a pipe, so a node, python, go and rust caller run identical code.

Install a client package for one target, or a runtime package to drive several targets from one process.

## Clients

| Target | npm | PyPI | Go |
| --- | --- | --- | --- |
| `akamai` | [`@proofofbot/client-akamai`](https://www.npmjs.com/package/@proofofbot/client-akamai) | [`wre-client-akamai`](https://pypi.org/project/wre-client-akamai/) | `github.com/proofofbots/web-re-toolkit/packages/go/clients/akamai` |
| `altcha` | [`@proofofbot/client-altcha`](https://www.npmjs.com/package/@proofofbot/client-altcha) | [`wre-client-altcha`](https://pypi.org/project/wre-client-altcha/) | `github.com/proofofbots/web-re-toolkit/packages/go/clients/altcha` |
| `kasada` | [`@proofofbot/client-kasada`](https://www.npmjs.com/package/@proofofbot/client-kasada) | [`wre-client-kasada`](https://pypi.org/project/wre-client-kasada/) | `github.com/proofofbots/web-re-toolkit/packages/go/clients/kasada` |
| `example` | [`@proofofbot/client-example`](https://www.npmjs.com/package/@proofofbot/client-example) | [`wre-client-example`](https://pypi.org/project/wre-client-example/) | `github.com/proofofbots/web-re-toolkit/packages/go/clients/example` |

`example` is a worked adapter against a demo collector, not a real service. Use it to learn the API.

## Runtimes

Use a runtime directly to drive a binary that has no generated package, or several targets from one process.

| Language | Package | Guide |
| --- | --- | --- |
| Node.js | [`@proofofbot/runtime`](https://www.npmjs.com/package/@proofofbot/runtime) | [Node.js](/web-re-toolkit/packages/node/) |
| Python | [`wre-runtime`](https://pypi.org/project/wre-runtime/) | [Python](/web-re-toolkit/packages/python/) |
| Go | `github.com/proofofbots/web-re-toolkit/packages/go/wre` | [Go](/web-re-toolkit/packages/go/) |
| Rust | `wre-client` in this repository | [Rust](/web-re-toolkit/packages/rust/) |

Rust packages are generated into `dist/<bundle>/packages/rust` as `wre-sdk-<target>`. They are not published to crates.io. Depend on them by path, or use `wre-client` directly.

## Ops by target

An op is one callable on a session. `wre client describe <target>` prints the current set with parameter shapes.

| Target | Ops |
| --- | --- |
| `akamai` | `info`, `discover`, `solve`, `payload`, `post`, `request`, `page`, `cookies`, `pow`, `pixel`, `reset` |
| `kasada` | `info`, `discover`, `solve`, `request`, `loader`, `pow`, `payload`, `vector`, `report`, `cookies`, `misses`, `reset` |
| `altcha` | `info`, `challenge`, `solve`, `derive_key`, `verify`, `create_challenge`, `his`, `deobfuscate`, `server_signature`, `submit` |
| `example` | `roles`, `build`, `hash`, `encode`, `seal`, `payload`, `solve`, `stall`, `submit` |

## The binary

A client package declares the `wred` binary for your platform as an optional dependency, or downloads and hash-checks it into a cache on first use. Set `WRE_BINARY` to an absolute path to use a local build. That path skips the SHA-256 check.

Platforms built for every release:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-pc-windows-msvc`

Each package pins a schema hash. The hash is checked at connect time, and a mismatch fails the connect call, because it means the package and the installed binary disagree about the callable surface.

## Error kinds

Every binding raises the same nine kinds. Branch on the kind, never on the message.

| Kind | Meaning | Retryable by default |
| --- | --- | --- |
| `bad_input` | Parameters failed validation. | no |
| `unsupported` | No such op, target or option in this build. | no |
| `target_drift` | The shipped script no longer matches what the client expects. | no |
| `blocked` | The service answered with a challenge or a block. | yes |
| `timeout` | The deadline passed. | yes |
| `cancelled` | The caller cancelled the call. | no |
| `resource` | Something the client needs is missing or exhausted. | yes |
| `protocol` | Malformed frame, envelope or version mismatch. | no |
| `internal` | Unclassified. | no |

The `retryable` field on an error reflects what the host sent for that specific failure and can differ from this table.

## Environment variables

| Variable | Effect |
| --- | --- |
| `WRE_BINARY` | Absolute path to a `wred` binary. Skips resolution and the hash check. |
| `WRE_STDERR` | `inherit` shows the sidecar log, `ignore` discards it. Overrides the code-level option. |
| `WRE_DIAG` | `always` records a diagnostics report for every call, `off` records none. |
| `WRE_CACHE_DIR` | Where downloaded binaries are cached. |
