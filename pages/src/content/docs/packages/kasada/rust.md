---
title: Kasada from Rust
description: Depend on the generated wre-sdk-kasada crate, answer an interrogation, and fetch the page again through the same session.
---

Rust packages are generated into `dist/<bundle>/packages/rust` and are not published to crates.io, so depend on one by path:

```bash
wre client package --bundle default
```

```toml
[dependencies]
wre-sdk-kasada = { path = "dist/default/packages/rust/kasada" }
```

A Kasada session mounts a graph profile. One is compiled into the binary, so there is nothing to capture before the first run. Capture your own with `wre sandbox capture --graph --open` and pass its id as `profile` when you want a graph that is not shared with every other user, or one from a different browser.

```rust
use wre_sdk_kasada::{Client, KasadaConfig, OpenOptions, SolveInput};

let config = KasadaConfig {
    page_url: Some("https://acme.example/buy".into()),
    ..Default::default()
};

let client = Client::open(&config, OpenOptions::default())?;
let solved = client.solve(&SolveInput::default())?;
println!("{} {}", solved.verdict, solved.payload_bytes);
client.close()?;
```

The token is bound to the `KP_UIDz` cookie the edge set on the interrogation, so solve against the url you actually want, then send everything else through the same client.

## A full run

Open one session, report what the page is serving, answer the interrogation, print how many of its own checks the agent flagged, then fetch the page again through the same session and list what came back. A session that never answered gets the interrogation instead of the page, which is the point of the comparison.

```rust
use std::collections::BTreeSet;

use regex::Regex;
use wre_sdk_kasada::{Client, DiscoverInput, KasadaConfig, OpenOptions, RequestInput, SolveInput};

fn listings(html: &str) -> Vec<String> {
    let pattern = Regex::new(r#"href="(/property-[^"]+)""#).unwrap();
    let mut found = BTreeSet::new();
    for capture in pattern.captures_iter(html) {
        found.insert(capture[1].to_string());
    }
    found.into_iter().collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let page = std::env::var("PAGE")
        .unwrap_or_else(|_| "https://www.realestate.com.au/buy/in-sydney,+nsw/list-1".into());

    let config = KasadaConfig {
        page_url: Some(page.clone()),
        ..Default::default()
    };

    let client = Client::open(&config, OpenOptions::default())?;

    let surface = client.discover(&DiscoverInput::default())?;
    println!("{page} answered {}, protected {}", surface.status, surface.protected);

    if !surface.protected {
        println!("no interrogation is being served, nothing to solve");
    } else {
        let solved = client.solve(&SolveInput::default())?;
        println!(
            "verdict {}, clearance {}",
            solved.verdict,
            solved.clearance.as_deref().unwrap_or("")
        );
        println!("payload {} bytes in {} ms", solved.payload_bytes, solved.ms);

        let report = client.report()?;
        let flagged = report.flagged.as_array().map(Vec::len).unwrap_or(0);
        println!("the agent flagged {flagged} of its own checks");
    }

    let answered = client.request(&RequestInput {
        url: page.clone(),
        ..Default::default()
    })?;
    println!("page {}, {} bytes", answered.status, answered.bytes);

    let found = listings(&answered.body);
    println!("{} listings", found.len());
    for href in found.iter().take(10) {
        println!("  https://www.realestate.com.au{href}");
    }

    client.close()?;
    Ok(())
}
```

A failure comes back as a `ClientError` with a stable kind, so branch on the kind rather than the message. `wre-client` in this repository is the same surface without a generated crate, and is covered on the [Rust package page](/web-re-toolkit/packages/rust/). What the client does and what the config controls is in [The Kasada client](/web-re-toolkit/guides/kasada/).
