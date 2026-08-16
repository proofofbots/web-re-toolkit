---
title: Rust
description: Depend on a generated wre-sdk crate by path, or call the sidecar directly through wre-client.
---

Rust packages are generated into `dist/<bundle>/packages/rust` as `wre-sdk-<target>`. They are not published to crates.io, so depend on one by path:

```bash
wre client package --bundle default
```

```toml
[dependencies]
wre-sdk-akamai = { path = "dist/default/packages/rust/akamai" }
```

## Use

```rust
use wre_sdk_akamai::{AkamaiConfig, Client, OpenOptions, SolveInput};

let config = AkamaiConfig {
    page_url: Some("https://acme.example/".into()),
    ..Default::default()
};

let client = Client::open(&config, OpenOptions::default())?;
let solved = client.solve(&SolveInput::default())?;
println!("{:?}", solved.cookies);
client.close()?;
```

The binary is found through `WRE_BINARY`, then `WRE_WRED`, then `target/release/wred` upward from the working directory, then `PATH`.

## Without a generated crate

`wre-client` in this repository carries the `Client` trait, the op schema and the sidecar protocol, plus a rust consumer. Use it to drive a binary that has no generated package, or several targets from one process.

Writing a new client is the other side of the same crate. [Headless clients](/web-re-toolkit/guides/clients/) is the authoring guide.
