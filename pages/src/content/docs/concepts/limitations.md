---
title: Limitations
description: What the deobfuscation pipeline, the lifter, the transport emulation and the sandbox do not do.
---

## Deobfuscation

The passes produce a reconstruction, not a byte-equivalent program. Renaming and dead-binding removal change observable globals in a classic script, so `remove_unused` is off by default and on in the `readable` preset.

## The lifter

The lifter emits structured control flow when the control flow graph is reducible, and falls back to a labelled dispatch loop when it is not. Both are correct. Unknown opcodes are lifted as `opN(args)` calls and reported, not guessed.

## Transport

`wre-net` sends requests through `wreq`, so the ClientHello, HTTP/2 settings and header order match the browser profile the client emulates. A client built from a user agent picks the nearest profile for that agent, so the transport and the header agree.

The profile set is whatever `wreq-util` ships, so a browser release is only reachable once it lands upstream. A profile is one build's snapshot: it carries no per-installation variation, and nothing here hides a mismatch between the emulated client and the payload it sends.

## The sandbox

The property profile the sensor clients mount has no layout. A target that creates an element and measures it gets zero. A graph profile carries a measured layout table and answers from it, for the elements the capture measured; anything outside that set records a miss.

The realm is a real engine with a fake environment. A target that reaches for something the capture did not reach gets `undefined`, which appears in the miss log. A graph replays what was walked and invents nothing, so a surface missing from the capture is missing from the realm.

Canvas and WebGL readbacks come from a recorded reply table keyed by the operations that produced them. Without a table for the operations a target performs, the readback is blank and the run records a miss.

## Build comparison

Similarity between two builds is a structural estimate, not a proof. A function reported as edited at 90 percent shared structure still has to be read.
