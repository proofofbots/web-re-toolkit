#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

wre="$root/target/debug/wre"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

step() { printf '\n=== %s\n' "$1"; }

step "passes"
"$wre" passes | head -5

step "markers"
"$wre" markers | head -5

step "init and check a target"
"$wre" --root "$work" init demo --url https://demo.example.test/ >/dev/null
"$wre" --root "$work" check demo

step "deobfuscate an obfuscated sample"
cat > "$work/sample.js" <<'JS'
var _0x4f = {
  'kQx': function (a, b) { return a + b; },
  'wPz': function (a, b) { return a === b; }
};
function _0xmain(_0xarg) {
  var _0xel = document['createElement']('canvas');
  var _0xctx = _0xel['getContext']('2d');
  _0xctx && _0xctx['fillRect'](0, 0, !0 ? 1 : 2, 3);
  var _0xname = _0x4f['kQx']('ca', 'nvas');
  return _0x4f['wPz'](_0xname, _0xarg) ? _0xctx : null;
}
JS
"$wre" deobf "$work/sample.js" --rename --out "$work/sample.clean.js" --stats
sed -n '1,12p' "$work/sample.clean.js"

step "surface index"
"$wre" surface "$work/sample.clean.js" --limit 5

step "mount and borrow a primitive"
cat > "$work/target.js" <<'JS'
function fnv1a(text) {
  var hash = 0x811c9dc5;
  for (var i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash >>> 0;
}
JS
mkdir -p "$work/targets"
cat > "$work/targets/live.toml" <<'TOML'
name = "live"

[deobfuscate]
source_kind = "script"

[[live.signatures]]
role = "hash"
pattern = "0x811c9dc5|2166136261"
params = 1
TOML
"$wre" --root "$work" mount "$work/target.js" --target live
"$wre" --root "$work" mount "$work/target.js" --target live --role hash --args '["hello"]'

step "vm discovery"
cat > "$work/vm.js" <<'JS'
var CODE = [0, 5, 1, 2, 3, 3, 9];
var HANDLERS = [
  function loadConst(state, read, store) { store(0, read()); },
  function add(state, read, store) { var a = read(); var b = read(); store(2, a + b); },
  function branchIfFalsy(state, read, store) { if (!read()) { state.pc = read(); } },
  function halt(state, read, store) { state.done = true; state.result = read(); }
];
function run(state, read, store) {
  while (!state.done) {
    var opcode = CODE[state.pc++];
    HANDLERS[opcode](state, read, store);
  }
}
JS
"$wre" vm discover "$work/vm.js"

step "vm handler probing"
cat > "$work/frame.js" <<'JS'
function (recorder, options) {
  var state = new Proxy({}, {
    get: function (target, key) {
      if (key === "pc") return 0;
      if (key === "done") return false;
      return recorder.sentinel("state." + String(key));
    },
    set: function (target, key, value) {
      if (key === "pc") { recorder.jump(value); } else { recorder.write(String(key)); }
      return true;
    }
  });
  return [state, function () { return recorder.read(); }, function (slot) { recorder.write(slot); }];
}
JS
"$wre" vm probe "$work/vm.js" --table HANDLERS --frame "$work/frame.js"

step "vm lift"
cat > "$work/program.json" <<'JSON'
{
  "entry": 0,
  "instructions": {
    "0": { "pc": 0, "opcode": 0, "next": 2, "kind": { "op": "load-const" }, "writes": [0], "operands": [{ "kind": "int", "value": 5 }], "label": "loadConst" },
    "2": { "pc": 2, "opcode": 1, "next": 4, "kind": { "op": "binary", "operator": "+" }, "writes": [2], "operands": [{ "kind": "register", "index": 0 }, { "kind": "int", "value": 3 }], "label": "add" },
    "4": { "pc": 4, "opcode": 2, "next": 6, "kind": { "op": "branch" }, "conditional": true, "operands": [{ "kind": "register", "index": 2 }], "jumps": [{ "pc": 7, "taken_when": false }], "label": "branchIfFalsy" },
    "6": { "pc": 6, "opcode": 0, "next": 7, "kind": { "op": "load-const" }, "writes": [2], "operands": [{ "kind": "text", "value": "fallback" }] },
    "7": { "pc": 7, "opcode": 3, "next": 8, "kind": { "op": "return" }, "terminal": true, "operands": [{ "kind": "register", "index": 2 }], "label": "halt" }
  }
}
JSON
"$wre" vm listing "$work/program.json"
"$wre" vm cfg "$work/program.json"
"$wre" vm lift "$work/program.json"

step "wire round trip, diff, forge, schema"
printf '{"s1":{"v":1},"s7":{"v":10},"c":"key","id":"3f2504e0-4f89-41d3-9a0c-0305e82c3301"}' > "$work/a.json"
printf '{"s1":{"v":1},"s7":{"v":32},"c":"key","id":"3f2504e0-4f89-41d3-9a0c-0305e82c3302"}' > "$work/b.json"
"$wre" wire roundtrip "$work/a.json" --codec json
"$wre" wire diff "$work/a.json" "$work/b.json"
"$wre" wire forge "$work/a.json" --set 's7.v=32' --set 'c="other"' | head -12
"$wre" wire schema "$work/a.json" "$work/b.json"

step "environment snapshot replay"
cat > "$work/collect.js" <<'JS'
globalThis.report = navigator.platform + " " + screen.width + " " + navigator.languages.join("/");
JS
"$wre" env run "$work/collect.js" --expression 'report'

step "transport fingerprints"
python3 - "$work" <<'PY'
import sys, pathlib
work = pathlib.Path(sys.argv[1])
frames = bytearray(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")

def frame(kind, flags, stream, payload):
    out = bytearray()
    out += len(payload).to_bytes(3, "big")
    out.append(kind)
    out.append(flags)
    out += stream.to_bytes(4, "big")
    out += payload
    return out

settings = bytearray()
for ident, value in ((1, 65536), (2, 0), (4, 6291456), (6, 262144)):
    settings += ident.to_bytes(2, "big") + value.to_bytes(4, "big")

frames += frame(0x4, 0, 0, settings)
frames += frame(0x8, 0, 0, (15663105).to_bytes(4, "big"))
frames += frame(0x1, 0x5, 1, bytes([0x82, 0x87, 0x84, 0x41, 0x8c, 0xf1, 0xe3, 0xc2, 0xe5, 0xf2, 0x3a, 0x6b, 0xa0, 0xab, 0x90, 0xf4, 0xff]))
(work / "h2.bin").write_bytes(bytes(frames))
PY
"$wre" tls h2 "$work/h2.bin"

step "acceptance"
"$wre" --root "$work" verify --target demo

printf '\nsmoke run finished\n'
