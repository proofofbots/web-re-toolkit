# Examples

Runnable scripts that drive a client through the sidecar, one per language. Each one reaches a live
endpoint.

Both need a `wred` binary:

```bash
cargo build -p wre-clientd
export WRE_BINARY="$PWD/target/debug/wred"
```

The Kasada scripts mount a graph profile. The binary carries one, so they run as they are. `wre sandbox capture --graph --open` records your own into `profiles/graph`, and the first one there wins over the bundled graph.

## Node

Point the example at the runtime in this repository, then run it:

```bash
mkdir -p examples/node/node_modules/@proofofbot
ln -sfn "$PWD/packages/node/runtime" examples/node/node_modules/@proofofbot/runtime
node examples/node/kasada-scrape.mjs
```

`PAGE` overrides the url, which defaults to a Sydney search on `realestate.com.au`.

## Python

```bash
PYTHONPATH=packages/python python3 examples/python/kasada_scrape.py
```

`PAGE` overrides the url here too.

## What they do

Both scripts open one Kasada session, report what the page is serving, answer the interrogation,
print how many of its own checks the agent flagged, then fetch the page again through the same
session and list what came back. A session that never answered gets the interrogation instead of the
page, which is the point of the comparison.
