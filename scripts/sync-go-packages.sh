#!/usr/bin/env bash
set -euo pipefail

bundle="${1:-default}"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

source_root="dist/$bundle/packages/go"
target_root="packages/go/clients"

if [ ! -d "$source_root" ]; then
  echo "no generated go packages in $source_root, run wre client package first" >&2
  exit 1
fi

mkdir -p "$target_root"

for dir in "$source_root"/*/; do
  name="$(basename "$dir")"
  destination="$target_root/$name"

  mkdir -p "$destination"
  cp "$dir"*.go "$destination/"
  cp "$dir"go.mod "$destination/"
  cp "$dir"README.md "$destination/" 2>/dev/null || true

  python3 - "$destination/go.mod" <<'PY'
import re
import sys

path = sys.argv[1]
text = open(path, encoding="utf-8").read()
text = re.sub(r"(?m)^replace .*\n", "", text)
text = re.sub(r"\n{3,}", "\n\n", text)
open(path, "w", encoding="utf-8").write(text)
PY

  echo "synced $destination"
done

echo
echo "go modules publish from the repository tree by tag, so commit these and then tag:"
echo "  git tag packages/go/wre/v<version>"
for dir in "$source_root"/*/; do
  echo "  git tag packages/go/clients/$(basename "$dir")/v<version>"
done
echo "  git push origin --tags"
