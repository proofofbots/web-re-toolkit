# wre

Go client for the [wre sidecar protocol](../../../docs/PROTOCOL.md). It spawns a `wred` binary, speaks the length-prefixed frame protocol over its stdin and stdout, and exposes sessions and ops as typed Go calls.

## Install

```
go get github.com/proofofbots/web-re-toolkit/packages/go/wre
```

## Usage

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

Set `WRE_BINARY` to an absolute path to skip the download cache and hash check. Without it, `ResolveBinary` looks in `WRE_CACHE_DIR` (or `XDG_CACHE_HOME`/`~/.cache`/`%LOCALAPPDATA%` depending on platform) under `bin/<version>/<triple>/wred`, downloading and verifying against the given SHA256 when a `BinarySpec.URL` is set.

## Errors

Every failure is a `*wre.Error` with a stable `Kind`. Branch on it with `wre.IsKind`:

| kind | meaning | retryable by default |
| --- | --- | --- |
| `bad_input` | parameters failed validation | no |
| `unsupported` | no such op, target or option in this build | no |
| `target_drift` | the shipped script no longer matches what the client expects | no |
| `blocked` | the service answered with a challenge or a block | yes |
| `timeout` | the deadline passed | yes |
| `cancelled` | the caller cancelled the context | no |
| `resource` | something the client needs is missing or exhausted | yes |
| `protocol` | malformed frame, envelope or version mismatch | no |
| `internal` | unclassified | no |

