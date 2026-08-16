---
title: Command reference
description: Every wre subcommand, what it does, and which command group it belongs to.
---

Run `wre <command> --help` for the flags of any command.

## Global options

| Option | Description |
| --- | --- |
| `--root <ROOT>` | Workspace root. Defaults to the nearest `wre.toml`, then `.git`. |
| `--json` | Print machine readable JSON instead of a table. |
| `--log <LOG>` | Log level. Default `info`. |

## Workspace

| Command | Description |
| --- | --- |
| `wre init <NAME>` | Write a target manifest to `targets/<name>.toml`. Takes `--url`. |
| `wre targets` | List the target manifests in this workspace. |
| `wre check <TARGET>` | Check a manifest without running anything. |

## Capture

| Command | Description |
| --- | --- |
| `wre discover <URL>` | Find a target's surface in a document, with no browser. |
| `wre browser` | Manage the shared Chrome instance. Takes `--start`, `--stop`, `--status`, `--headless`, `--port`. |
| `wre capture` | Record a browser run into a capture bundle. |
| `wre pin <NAME>` | Copy a capture into `captures/<name>` so it survives an artifacts wipe. |
| `wre show <PATH>` | Summarise a capture bundle. |

## Script

| Command | Description |
| --- | --- |
| `wre deobf <FILE>` | Run the deobfuscation pipeline over a file. |
| `wre beautify <FILE>` | Reformat a file without changing it. |
| `wre passes` | List the passes in the pipeline. |
| `wre surface <FILE>` | Report the browser surface each function reaches. |
| `wre mount <FILE>` | Find the target's own primitives and call them. |
| `wre integrity <FILE>` | Check, or restore, a script's hash of its own source. |
| `wre equivalent <A> <B>` | Check that a rewrite reaches for nothing the original did not. |

## Identification

| Command | Description |
| --- | --- |
| `wre locate <FILE>` | Find the target's roles by structure and behaviour rather than by name. |
| `wre drift <LOCK> <FILE>` | Report which locked roles moved in a newer build. |
| `wre builds <OLD> <NEW>` | Pair the functions of two builds and say what changed. |
| `wre align` | Align the slots of one build against another by value. |

## Virtual machine

| Command | Description |
| --- | --- |
| `wre vm discover <FILE>` | Look for a dispatch loop and a handler table. |
| `wre vm probe <FILE>` | Probe every handler in a table for its operand and register shape. |
| `wre vm listing <FILE>` | Print a decoded instruction stream. |
| `wre vm lift <FILE>` | Lift a decoded instruction stream to readable JavaScript. |
| `wre vm cfg <FILE>` | Report basic blocks, loops and reducibility. |
| `wre vm align <TRACE>` | Align a recorded trace to handler identities. |

## Payloads

| Command | Description |
| --- | --- |
| `wre wire open <BODY>` | Decode a body. |
| `wre wire seal <VALUE>` | Encode a value. |
| `wre wire roundtrip <BODY>` | Check that a body decodes and re-encodes byte for byte. |
| `wre wire diff <A> <B>` | Diff two payloads by address. |
| `wre wire forge <DONOR>` | Build a payload from a donor with fields replaced. |
| `wre wire schema <FILES>` | Infer a schema across several payloads. |
| `wre grade <BUILT>` | Grade a built payload against several real ones. |

## Environment and sandbox

| Command | Description |
| --- | --- |
| `wre env script` | Print the script that captures an environment snapshot. |
| `wre env snapshot` | Capture a snapshot from a live page. |
| `wre env run <SCRIPT>` | Run a script inside a realm materialised from a snapshot. |
| `wre sandbox list` | List the captured fingerprint profiles in the workspace. |
| `wre sandbox profile` | Print the browser surface that would be installed. |
| `wre sandbox check` | Mount the surface and check it looks like a real browser. |
| `wre sandbox capture` | Serve the capture page and store the profile the browser sends back. |
| `wre sandbox import <FILE>` | Store a profile captured with the page's download button. |

## Attribution

| Command | Description |
| --- | --- |
| `wre sweep` | Attribute payload addresses to knobs from recorded captures. |
| `wre pools` | Plan the pooled runs that attribute many markers in few loads. |
| `wre markers` | List the built in automation markers. Takes `--kind`. |
| `wre diff <A> <B>` | Diff two generated maps, ignoring counter renames. |
| `wre baseline <MAP>` | Save a generated map as a baseline. |
| `wre verify` | Run every check that needs no browser. |

## Transport

| Command | Description |
| --- | --- |
| `wre tls hello <FILE>` | Compute JA3 and JA4 from a raw ClientHello. |
| `wre tls h2 <FILE>` | Compute the HTTP/2 settings fingerprint from a raw frame stream. |

## Headless clients

| Command | Description |
| --- | --- |
| `wre client new <ID>` | Scaffold a client crate under `clients/<id>` and wire it into `wred`. |
| `wre client bundles` | List the bundles declared in `clients.toml`. |
| `wre client list` | List the targets compiled into a `wred` binary. |
| `wre client describe <ID>` | Print the ops, events and capabilities of a target. |
| `wre client schema` | Write the bundle descriptor the generators read. |
| `wre client build` | Cross build `wred` for a bundle into `dist/<bundle>/bin`. |
| `wre client package` | Generate the node, python, go and rust packages. |
| `wre client test` | Run a conformance suite against one or more bindings. |
| `wre client publish` | Print the commands that publish a bundle's packages. |
| `wre client diag <FILE>` | Summarise a diagnostics report. |
