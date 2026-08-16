---
title: A worked pass
description: Capture a protected page, open its script, call its primitives, lift its virtual machine, and check the payload round-trips.
---

Each stage writes to `artifacts/` and reads what the stage before it wrote. Run them in order the first time, then jump to whichever one you need.

## 1. Discover the surface

Find the script and endpoints a document names, with no browser:

```bash
wre discover https://acme.example/ --target acme
wre discover https://acme.example/ --target acme --fingerprint chrome_141:windows
```

## 2. Capture a run

```bash
wre capture --target acme --scripts
wre pin acme-2026-08-15
wre show captures/acme-2026-08-15
```

`--scripts` stores the script bodies alongside the requests. `pin` copies the bundle into `captures/`, where an `artifacts/` wipe cannot reach it.

## 3. Open the script

```bash
wre deobf artifacts/captures/.../collect.js --target acme --rename --stats
wre surface collect.clean.js
```

`deobf` runs a 26-pass pipeline to fixpoint, including control flow unflattening and evidence-based renaming. `surface` reports which browser surface each function reaches.

## 4. Call its primitives

```bash
wre mount collect.js --target acme
wre mount collect.js --target acme --role seal --args '[{"a":1}]'
```

`mount` loads the script into a V8 realm, matches the roles declared in the manifest, and hands you its own functions as callable handles. The first form lists what matched. The second calls one.

## 5. Lift the virtual machine

```bash
wre vm discover collect.js
wre vm probe collect.js --table HANDLERS --frame frame.js
wre vm lift program.json --out lifted.js
```

`probe` runs each handler against Proxy sentinels and records what it read, what it wrote, and whether it touched the program counter. Only the frame model, meaning how the VM calls its handlers, is per-target.

## 6. Work the payload

```bash
wre wire roundtrip body.bin --codec base64
wre wire diff before.json after.json
wre wire schema captures/*/payload.json
```

`roundtrip` opens a body, seals it again and compares bytes. A codec that cannot reproduce the original is reported as such.

## 7. Verify

```bash
wre verify --target acme --capture captures/acme-2026-08-15
```

Runs every check the manifest declares that needs no browser.

## After the vendor rebuilds

```bash
wre locate collect.js --target acme --lock targets/acme.lock
wre drift targets/acme.lock collect-new.js
wre builds collect-old.js collect-new.js

wre integrity collect.js --target acme
wre integrity collect.patched.js --target acme --resign
wre equivalent collect.js collect.clean.js
```

[Finding things again after a rebuild](/web-re-toolkit/guides/identification/) covers what the lock file holds and how the drift report reads.

## Grading and attribution

```bash
wre grade built.json --real capture-1.json capture-2.json
wre align --before a1.json a2.json --after b1.json b2.json

wre sandbox capture --open
wre sandbox check --all
wre markers --kind concealment
wre pools
```

Run the baseline twice before sweeping anything. Addresses that differ between two identical runs are noise, and every sweep result is reported with them removed.
