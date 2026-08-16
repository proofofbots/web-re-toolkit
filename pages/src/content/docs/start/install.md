---
title: Install
description: Build the wre binary from source, and tell it where Chrome is.
---

## Prerequisites

- Rust stable, with `cargo` on your `PATH`.
- Network access for the first build. The V8 crate pulls a prebuilt static library, so the first compile is slow.
- Chrome or Chromium, for anything that drives a browser. Deobfuscation, mounting, VM work and payload work need no browser.

## Build

```bash
git clone https://github.com/proofofbots/web-re-toolkit
cd web-re-toolkit
cargo build --release
```

## Verify

```bash
./target/release/wre --help
```

Put the binary on your `PATH` if you want to type `wre` instead of the full path:

```bash
cargo install --path crates/wre-cli
```

## Chrome

Chrome is located automatically on macOS, Linux and Windows. Set `WRE_CHROME` to an absolute path to override the search:

```bash
export WRE_CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
wre browser --status
```

## Run the tests

```bash
cargo test --workspace
bash scripts/smoke.sh
bash scripts/client-smoke.sh
```

`smoke.sh` runs the CLI end to end with no browser: it deobfuscates an obfuscated sample, mounts a target and calls its hash function, discovers and probes a toy VM, lifts an instruction stream, round-trips and diffs payloads, replays an environment snapshot, and computes an HTTP/2 fingerprint.

`client-smoke.sh` builds `wred`, runs the conformance suite through all four bindings, generates the packages, and calls the generated node, python and go packages against the built binary.

## Next steps

- [Set up a workspace](/web-re-toolkit/start/workspace/)
- [Run a worked pass](/web-re-toolkit/start/first-pass/)
