# The sidecar protocol

Headless clients are written once in Rust and compiled into one host binary, `wred`. Every language package talks to that binary over a pipe.

A binding that follows this protocol works against any bundle.

## Transport

The consumer spawns `wred --stdio` and speaks frames over its stdin and stdout. Its stderr carries human readable logs (`WRE_LOG` sets the level, default `info`) and is never part of the protocol.

`wred --socket <path>` serves the same protocol over a Unix domain socket for a shared daemon. There is no TCP listener.

## Frames

Every message is a length prefixed frame:

```text
u32be json_len | u32be bin_len | json | bin
```

The 8 byte prefix is followed by exactly `json_len` bytes of UTF-8 JSON, then exactly `bin_len` bytes of raw binary (usually zero). The JSON envelope states what the blob is; a reader must not guess. Frames larger than 64 MiB of JSON or 512 MiB of binary are rejected as corrupt.

Both directions use the same framing. Neither side may write a partial frame then write another.

## Envelopes

The JSON part is one of four objects, discriminated by `t`.

Request, consumer to host:

```json
{"t":"req","v":1,"id":7,"op":"solve","session":"s3","params":{"url":"https://acme.example/"},"deadline_ms":20000}
```

`session` is omitted for base ops that are not session scoped. `deadline_ms` is optional; its declared default applies when missing.

Response, host to consumer, exactly one per request:

```json
{"t":"res","v":1,"id":7,"ok":true,"result":{"body":"..."},"took_ms":31}
```

```json
{"t":"res","v":1,"id":7,"ok":false,"error":{"kind":"timeout","message":"solve ran past its deadline","retryable":true}}
```

Event, host to consumer, zero or more before the response, carrying the request's `id`:

```json
{"t":"evt","v":1,"id":7,"event":"progress","data":{"done":2,"total":3,"note":"sealing"}}
```

Cancel, consumer to host:

```json
{"t":"cancel","v":1,"id":7}
```

A cancelled request still gets a response, usually with `kind: "cancelled"`. Cancellation is cooperative: the host flips a flag the client polls between steps. A cancel for an unknown id is ignored.

Requests may be pipelined. Responses may arrive out of order; a binding correlates on `id` and must not assume ordering. Ids are chosen by the consumer and must be unique for the connection.

## Handshake

The first request must be `hello`. The reply:

```json
{
  "protocol": 1,
  "bundle": "default",
  "binary_version": "0.1.0",
  "toolkit_version": "0.1.0",
  "schema_hash": "0f3a9c2b1d4e5f60",
  "targets": ["example"],
  "workers": 4,
  "pid": 51234
}
```

A binding compares `protocol` against its own and fails immediately when they differ. `schema_hash` digests the callable surface, changing when an op, type or config changes but not when the version number does. A mismatch means the binary and package disagree, and must be reported as an error.

## Base ops

Answered by the host.

| op | session | params | result |
| --- | --- | --- | --- |
| `hello` | no | `{}` | the handshake object above |
| `describe` | no | `{}` | the bundle descriptor (same JSON as `wred --describe`) |
| `targets` | no | `{}` | list of target ids |
| `metrics` | no | `{}` | counters as a flat object of numbers |
| `open` | no | `{"target": "example", "config": {...}}` | `{"session":"s1","target":"example","worker":0,"ops":["solve",...]}` |
| `close` | yes | `{}` | `{"closed": true}` |
| `health` | yes | `{}` | `{"ok": true, "target": "example", "detail": {...}}` |
| `warmup` | yes | `{}` | `{"warm": true}` |
| `shutdown` | no | `{}` | `{"stopping": true}`, then the host closes the connection |
| `diag` | yes | `{"write": true, "events": true}` | `{"target":..., "session":..., "mode":..., "path":..., "report":{...}}` |

Any other op is routed to the client that owns the named session. Calling a target op without a session returns a `bad_input` error.

`config` is validated against the target's declared config shape before the client is built, with defaults filled in first. Unknown fields are rejected rather than ignored.

`open` takes an optional `diag` object that configures the session's diagnostics recorder:

```json
{"target":"example","config":{},"diag":{"mode":"on_error","dir":"/tmp/reports","include_params":false,"max_events":400,"keep_files":20}}
```

## Sessions

A session owns the expensive state: mounted V8 realm, cookies, counters, keypairs. Opening one costs the target's warmup; a binding should reuse sessions rather than open one per call.

The host pins a session to a worker thread and routes every call for it to that thread.

Sessions do not outlive the connection. When the consumer disconnects, the host closes every session it opened.

## Errors

`error.kind` is the stable part. Bindings map it to their idiom and branch on it; `message` is for humans.

| kind | meaning | retryable |
| --- | --- | --- |
| `bad_input` | parameters failed validation against the op schema | no |
| `unsupported` | no such op, target or option in this build | no |
| `target_drift` | shipped script no longer matches the client | no |
| `blocked` | service answered with a challenge or block | yes |
| `timeout` | the deadline passed | yes |
| `cancelled` | the caller cancelled | no |
| `resource` | something the client needs is missing or exhausted | yes |
| `protocol` | malformed frame, envelope or version mismatch | no |
| `internal` | unclassified | no |

`retryable` is carried on the wire. `detail` is free-form JSON; `target` and `op` are filled in where the host knows them.

## Events

Declared per target; two are conventional and any client may emit them:

- `log`: `{"level": "info", "text": "..."}`
- `progress`: `{"done": 2, "total": 3, "note": "sealing"}`

Events are advisory. A binding that drops them must still work.

## Diagnostics

Every session carries a recorder: a bounded ring of structured events (session open/close, one entry per call with outcome and duration, client breadcrumbs, log lines) plus facts the client declares once, such as build tag and script digest.

`mode` decides when that turns into a file. `off` records nothing, `on_error` writes a report when a call fails, `always` writes one after every call. Default is `on_error`. `WRE_DIAG`, `WRE_DIAG_DIR` and `WRE_DIAG_PARAMS` override it process-wide.

A report is a single JSON file named `<target>-<utc stamp>-<session>.diag.json`, written under `<state>/diagnostics` unless `dir` says otherwise, with the oldest pruned once `keep_files` is exceeded. When a failing call produces one, the error's `detail.diagnostics` carries the path.

The report holds handshake facts, target and client version, scrubbed config, host environment, call counters, event ring, failure, and a `client` section: mounted roles, realm console and error records, state a solver kept. Values under keys that look secret (`proxy`, `token`, `authorization`, `cookie`, `key`, and similar) become a digest and length; long strings are truncated, sha256 kept. Call parameters are summarised as key names and digests unless `include_params` is on.

`diag` fetches the report over the protocol without waiting for failure, and writes it when `write` is true.

## The shape language

Op parameters, results, config and event data are described with a small type IR rather than JSON Schema. A shape is a JSON object with a `kind` field:

| kind | extra fields | JSON encoding |
| --- | --- | --- |
| `unit` | | `null` |
| `bool` | | boolean |
| `int` | | number with no fractional part |
| `float` | | number |
| `str` | | string |
| `bytes` | | base64 string |
| `json` | | anything |
| `list` | `of` | array |
| `map` | `of` | object with string keys |
| `optional` | `of` | the inner encoding or `null` |
| `enum` | `name`, `variants` | one of the variant strings |
| `object` | `name`, `fields` | object |
| `ref` | `name` | whatever the named type encodes as |

A field is `{"name": ..., "shape": ..., "summary": ..., "default": ...}`. A field is required when it has no default and its shape is not `optional`. Named `object` and `enum` shapes become named types in generated packages; `types` in the descriptor holds every named type reachable from config, ops and events.

## The descriptor

`wred --describe` prints:

```json
{
  "protocol": 1,
  "bundle": "default",
  "toolkit_version": "0.1.0",
  "binary_version": "0.1.0",
  "clients": [
    {
      "id": "example",
      "version": "0.1.0",
      "summary": "...",
      "capabilities": {
        "needs_v8": true,
        "needs_chrome": false,
        "needs_network": true,
        "stateful": true,
        "concurrency": "per_session",
        "warmup_ms": 150
      },
      "config": { "kind": "object", "name": "ExampleConfig", "fields": [...] },
      "ops": [
        {
          "name": "solve",
          "summary": "...",
          "params": { "kind": "ref", "name": "Facts" },
          "returns": { "kind": "object", "name": "Solved", "fields": [...] },
          "streams": ["progress"],
          "deadline_ms": 20000
        }
      ],
      "events": [ { "name": "progress", "data": {...} } ],
      "types": { "Facts": {...}, "Solved": {...} }
    }
  ]
}
```

`capabilities` is what the host needs before the client can run. `needs_chrome` is checked by `health` rather than the first call that wants a browser.

## Binary resolution in a package

Every generated package resolves the binary in this order:

1. `WRE_BINARY`, an absolute path to a `wred`. Used by air-gapped installs and anyone testing a local build.
2. The binary shipped inside the package, for npm and Python wheels.
3. The download cache, for Go: `$WRE_CACHE_DIR` or `$XDG_CACHE_HOME/wre` or `~/.cache/wre` on Unix, `%LOCALAPPDATA%\wre` on Windows, under `bin/<version>/<triple>/wred`.

Cases 2 and 3 verify the binary's sha256 against the manifest before executing it, and refuse to run on a mismatch. Case 1 skips the check.

Platform triples are the Rust ones:

| triple | node `process.platform`/`arch` | python `sysconfig.get_platform()` prefix / `platform.machine()` | go `GOOS`/`GOARCH` |
| --- | --- | --- | --- |
| `aarch64-apple-darwin` | `darwin`/`arm64` | `macosx` / `arm64` | `darwin`/`arm64` |
| `x86_64-apple-darwin` | `darwin`/`x64` | `macosx` / `x86_64` | `darwin`/`amd64` |
| `x86_64-unknown-linux-gnu` | `linux`/`x64` | `linux` / `x86_64` | `linux`/`amd64` |
| `aarch64-unknown-linux-gnu` | `linux`/`arm64` | `linux` / `aarch64` | `linux`/`arm64` |
| `x86_64-pc-windows-msvc` | `win32`/`x64` | `win` / `AMD64` | `windows`/`amd64` |

## What a binding must do

- Spawn the binary with `--stdio`, keep stderr attached to the parent unless the caller asks otherwise.
- Read frames on a background thread or task and correlate responses by `id`.
- Send `hello` first and fail on a protocol or schema mismatch.
- Expose sessions as objects that reuse the connection, and close them on scope exit.
- Turn `error.kind` into the language's error type.
- Surface events through a callback or an async iterator, and keep working when nobody listens.
- Kill the child on close, and fail every in-flight request when the child dies.
- Expose `diag` for producing one file to send back.
