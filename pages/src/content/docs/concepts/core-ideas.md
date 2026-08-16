---
title: Core ideas
description: The four decisions the toolkit is built on. Borrow the target's own code, identify by structure, probe handlers concolically, and subtract the noise floor.
---

## Borrow, do not reimplement

A target's crypto is already written and already correct. `wre-live` mounts the shipped script in a V8 realm and hands you its own functions as callable handles, so your decoder cannot drift from the build. Roles are matched by a regex against each top-level function's source, declared in the manifest:

```toml
[[live.signatures]]
role = "hash"
pattern = "0x811c9dc5|2166136261"
params = 1
```

## Concolic handler probing

For a custom VM whose opcode handlers are real JavaScript functions, you do not need to read them. Run each handler against Proxy sentinels standing in for the registers and the operand reader, and record what it read, what it wrote, and whether it touched the program counter.

Run it again with the first operand forced falsy and diff the two runs. A handler that behaves differently is a conditional branch. Only the frame model, meaning how the VM calls its handlers, is per-target, and it is about twenty lines of JavaScript.

## Handler identity beats opcode numbers

Protections permute the opcode table per build. Keying a trace on which handler function ran, rather than on the opcode number, makes the permutation irrelevant and recovers the mapping between two builds:

```bash
wre vm align trace.json --against old-trace.json
```

## Snapshot the browser instead of writing DOM stubs

`wre env snapshot` walks the real object graph into JSON. `wre env run` rebuilds it lazily inside a realm. Surfaces that cannot be faked in a headless realm route to a host bridge or a replay table.

## Identity survives a rebuild

Nothing important is found by matching the text of one build. A role is located by scoring several weak signals against a name blind normalisation of the abstract syntax tree: structural shape, magic constants, the property names it reaches, its position in the call graph, and where it matters, what it returns when you actually call it.

The result is written to a lock file, and the next build is diffed against that lock, so a rebuild produces a report of what moved rather than a pattern that silently stops matching. [Finding things again after a rebuild](/web-re-toolkit/guides/identification/) covers it.

## Native shapes, replayed values

The sandbox installs its browser surface as real V8 bindings, so accessors are native functions and wrong receivers throw `Illegal invocation`. The document, timers and events sit on top of that, and the functions they add report as native through a V8-level `toString`, which is a narrower lie than a JavaScript patch over the whole surface.

The values come from `profiles/`, one file per real device captured with `wre sandbox capture`. Nothing is generated. [The browser surface](/web-re-toolkit/guides/sandbox/) covers it.

## Attribute many facts in few runs

Testing 64 automation markers one at a time is 64 page loads. A pooled design plants them in groups so that each marker sits in a unique combination of runs, which takes 7. A pooled verdict is then confirmed by planting that one marker alone, because two markers together can produce the pattern of a third.

## Subtract the noise floor

Run the baseline twice before sweeping anything. Addresses that differ between two identical runs are noise, and every sweep result is reported with them removed.

## Every decode is checked by re-encoding

`verify_roundtrip` opens a body, seals it again, and compares bytes. A codec that cannot reproduce the original is reported as such.
