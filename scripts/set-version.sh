#!/usr/bin/env bash
set -euo pipefail

version="${1:-}"

if [ -z "$version" ]; then
  echo "usage: set-version.sh <version>" >&2
  exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

python3 - "$version" <<'PY'
import re
import sys

version = sys.argv[1]

if not re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.\-]+)?", version):
    raise SystemExit(f"{version} is not a semantic version")

path = "packages/node/runtime/package.json"
text = open(path, encoding="utf-8").read()
text = re.sub(r'"version":\s*"[^"]+"', f'"version": "{version}"', text, count=1)
open(path, "w", encoding="utf-8").write(text)

path = "packages/python/pyproject.toml"
text = open(path, encoding="utf-8").read()
text = re.sub(r'(?m)^version = "[^"]+"', f'version = "{version}"', text, count=1)
open(path, "w", encoding="utf-8").write(text)

path = "clients.toml"
text = open(path, encoding="utf-8").read()
text = re.sub(r'(?m)^version = "[^"]+"', f'version = "{version}"', text, count=1)
text = re.sub(r'(?m)^node_runtime = "[^"]+"', f'node_runtime = "^{version}"', text, count=1)
text = re.sub(r'(?m)^python_runtime = "[^"]+"', f'python_runtime = ">={version}"', text, count=1)
open(path, "w", encoding="utf-8").write(text)

print(f"set {version} in packages/node/runtime/package.json, packages/python/pyproject.toml and clients.toml")
PY
