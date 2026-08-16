---
title: Workspace layout
description: What a wre workspace holds, which directories survive a wipe, and what lives in a target manifest.
---

A workspace is any directory containing `wre.toml`. Every command resolves paths against it, so you can run `wre` from anywhere inside the tree.

```
my-research/
  wre.toml              marks the workspace root
  targets/acme.toml     the adapter, the only per-target file
  artifacts/            scratch, not committed
  captures/             pinned captures that survive an artifacts wipe
  reference/            generated tables and baselines
```

Create one:

```bash
wre init acme --url https://acme.example/
```

Pass `--root` to point a single command at a different workspace. Without it, `wre` walks up for the nearest `wre.toml`, then for `.git`.

## Artifacts and captures

`artifacts/` is scratch. Every capture, deobfuscated file and probe record lands there, and it is safe to delete.

`captures/` is what you keep. Promote a capture out of scratch before wiping:

```bash
wre pin acme-2026-08-15
wre show captures/acme-2026-08-15
```

## The target manifest

One TOML file per target under `targets/`, and it is the only per-target file. It holds:

- discovery patterns for finding the script and endpoints in a document
- primitive signatures and source patches for mounting
- the VM frame model and opcode labels
- the codec choice and field labels
- knob definitions for sweeps
- extra probe surfaces
- the check list for `wre verify`

Naming heuristics, coherence rules and opcode semantics are data in the manifest too. They look like code and they are tables.

A live signature declares a role and how to recognise it:

```toml
[[live.signatures]]
role = "hash"
pattern = "0x811c9dc5|2166136261"
params = 1
```

Check a manifest without running anything:

```bash
wre check acme
wre targets
```

## Next steps

- [Run a worked pass](/web-re-toolkit/start/first-pass/)
- [Command reference](/web-re-toolkit/reference/cli/)
