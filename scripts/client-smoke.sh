#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

wre="$root/target/debug/wre"
wred="$root/target/debug/wred"
triple="$(rustc -vV | sed -n 's/^host: //p')"
packages="$root/dist/default/packages"

export WRE_BINARY="$wred"

step() { printf '\n=== %s\n' "$1"; }
have() { command -v "$1" >/dev/null 2>&1; }

step "build the host and the cli"
cargo build --quiet -p wre-clientd -p wre-cli

step "the binary describes itself"
"$wred" --targets
"$wre" client list --bin "$wred"

step "conformance through every binding that has a toolchain"
languages="rust"
have node && languages="$languages,node"
have python3 && languages="$languages,python"
have go && languages="$languages,go"
"$wre" client test --bin "$wred" --lang "$languages"

step "stage the binary and generate the packages"
"$wre" client build --debug --platform "$triple"
"$wre" client package --bundle default

if have node; then
  step "the generated node package"
  mkdir -p "$packages/node/example/node_modules/@proofofbot"
  ln -sfn "$root/packages/node/runtime" "$packages/node/example/node_modules/@proofofbot/runtime"

  node --input-type=module -e "
    const mod = await import('$packages/node/example/index.js');
    const client = await mod.ExampleClient.open({ clock_ms: 1700000000000, seed: 7 });
    const hash = await client.hash({ text: 'abc' });
    const solved = await client.solve({ url: 'https://demo.example.test/' });
    await client.close();
    console.log('hash', hash.hex, 'digest', solved.digest, 'schema', mod.SCHEMA_HASH);
  "
fi

if have python3; then
  step "the generated python package"
  PYTHONPATH="$root/packages/python:$packages/python/example" python3 -c "
from wre_client_example import ExampleClient, SCHEMA_HASH

with ExampleClient.open({'clock_ms': 1700000000000, 'seed': 7}) as client:
    print('hash', client.hash({'text': 'abc'})['hex'], 'digest', client.solve({'url': 'https://demo.example.test/'})['digest'], 'schema', SCHEMA_HASH)
"
fi

if have go; then
  step "the generated go package"
  mkdir -p "$packages/go/example/smoke"
  cat > "$packages/go/example/smoke/main.go" <<'GO'
package main

import (
	"context"
	"fmt"
	"os"

	client "github.com/proofofbots/web-re-toolkit/packages/go/clients/example"
)

func main() {
	ctx := context.Background()
	clock := int64(1700000000000)
	seed := int64(7)

	handle, err := client.Open(ctx, &client.ExampleConfig{ClockMs: &clock, Seed: &seed}, client.OpenOptions{})
	if err != nil {
		fmt.Println("open failed:", err)
		os.Exit(1)
	}
	defer handle.Close(ctx)

	hash, err := handle.Hash(ctx, client.HashInput{Text: "abc"})
	if err != nil {
		fmt.Println("hash failed:", err)
		os.Exit(1)
	}

	solved, err := handle.Solve(ctx, client.Facts{URL: "https://demo.example.test/"})
	if err != nil {
		fmt.Println("solve failed:", err)
		os.Exit(1)
	}

	fmt.Println("hash", hash.Hex, "digest", solved.Digest, "schema", client.SchemaHash)
}
GO
  (cd "$packages/go/example" && go run ./smoke)
  rm -rf "$packages/go/example/smoke"
fi

step "a failing call writes one diagnostics report"
report="$("$wre" --json client test example --bin "$wred" --suite scripts/fixtures/client-failure.json 2>/dev/null || true)"
newest="$(ls -t artifacts/clients/diagnostics/*.diag.json 2>/dev/null | head -1 || true)"

if [ -n "$newest" ]; then
  "$wre" client diag "$newest" | head -12
else
  echo "no report was written"
  exit 1
fi

printf '\nclient smoke passed\n'
