---
title: Go
description: Install a generated client module or the runtime, open a session, and call a target's ops from Go with typed structs.
---

## Install a client

```bash
go get github.com/proofofbots/web-re-toolkit/packages/go/clients/akamai
```

Go 1.21 or later. Go modules publish from the repository tree by tag, so the module version follows the release tag.

```go
ctx := context.Background()

page := "https://acme.example/"
client, err := clientakamai.Open(ctx, &clientakamai.AkamaiConfig{PageURL: &page}, clientakamai.OpenOptions{})
if err != nil {
	log.Fatal(err)
}
defer client.Close(ctx)

solved, err := client.Solve(ctx, clientakamai.SolveInput{})
if err != nil {
	log.Fatal(err)
}
fmt.Println(string(solved.Cookies))
```

Each op is a method with typed input and result structs, generated from the descriptor.

## Install the runtime

Use the runtime to drive a binary that has no generated package, or several targets from one process.

```bash
go get github.com/proofofbots/web-re-toolkit/packages/go/wre
```

```go
ctx := context.Background()

sc, err := wre.Connect(ctx, wre.Options{
	Binary:           "/path/to/wred",
	ExpectSchemaHash: "0f3a9c2b1d4e5f60",
})
if err != nil {
	log.Fatal(err)
}
defer sc.Close()

sess, err := sc.Open(ctx, "example", map[string]any{"headless": true})
if err != nil {
	log.Fatal(err)
}
defer sess.Close(ctx)

var solved struct {
	Body string `json:"body"`
}
err = sess.Call(ctx, "solve", map[string]any{"url": "https://acme.example/"}, &solved)
if err != nil {
	if wre.IsKind(err, wre.KindBlocked) {
		log.Println("challenged, retry later")
	}
	log.Fatal(err)
}
fmt.Println(solved.Body)
```

## Events

Events are correlated by call id and delivered to one callback for the whole connection:

```go
sc, err := wre.Connect(ctx, wre.Options{
	Binary: "/path/to/wred",
	OnEvent: func(id uint64, event string, data json.RawMessage) {
		log.Printf("call %d %s %s", id, event, data)
	},
})
```

The callback runs on the reader goroutine, so keep it short and do not call back into the sidecar from it.

## Deadlines and cancellation

The context deadline travels on the wire, so the sidecar stops the work rather than the caller abandoning the call. A context that expires yields `KindTimeout`, one that is cancelled yields `KindCancelled`, and both send a cancel frame.

```go
ctx, cancel := context.WithTimeout(ctx, 20*time.Second)
defer cancel()
```

## Binary resolution

Set `WRE_BINARY` to an absolute path to skip the download cache and hash check. Without it, `ResolveBinary` looks in `WRE_CACHE_DIR` (or `XDG_CACHE_HOME`, `~/.cache`, `%LOCALAPPDATA%` depending on platform) under `bin/<version>/<triple>/wred`, downloading and verifying against the given SHA-256 when a `BinarySpec.URL` is set.

## Errors

Every failure is a `*wre.Error` with a stable `Kind`. Branch on it with `wre.IsKind`:

| Kind | Constant | Retryable by default |
| --- | --- | --- |
| `bad_input` | `wre.KindBadInput` | no |
| `unsupported` | `wre.KindUnsupported` | no |
| `target_drift` | `wre.KindTargetDrift` | no |
| `blocked` | `wre.KindBlocked` | yes |
| `timeout` | `wre.KindTimeout` | yes |
| `cancelled` | `wre.KindCancelled` | no |
| `resource` | `wre.KindResource` | yes |
| `protocol` | `wre.KindProtocol` | no |
| `internal` | `wre.KindInternal` | no |

`err.Retryable` reflects what the host sent for that specific failure and can differ from the table.

## Sidecar output and diagnostics

The sidecar writes its log to its own stderr, which is discarded by default so a library does not print into a host process. Set `Options.Stderr` to `os.Stderr` to see it. `WRE_STDERR=inherit` or `WRE_STDERR=ignore` overrides that field, which is the way to turn the log on in a deployed binary without touching code.

A failed call writes a report and puts its path in the error detail. `WRE_DIAG=always` records every call, `WRE_DIAG=off` records none, and a `diag` call with `{"write": true, "events": true}` writes one on demand.
