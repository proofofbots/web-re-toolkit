---
title: Akamai from Rust
description: Depend on the generated wre-sdk-akamai crate, warm a session against a protected page, and post a form through the same cookie jar.
---

Rust packages are generated into `dist/<bundle>/packages/rust` and are not published to crates.io, so depend on one by path:

```bash
wre client package --bundle default
```

```toml
[dependencies]
wre-sdk-akamai = { path = "dist/default/packages/rust/akamai" }
```

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

## A full run

Warm a session against a protected login page, read the antiforgery token out of the page the session already loaded, and post a form through the same jar.

```rust
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use wre_sdk_akamai::{
    AkamaiConfig, Client, DiscoverInput, OpenOptions, RequestInput, SolveInput,
};

const PAGE: &str = "https://login.xero.com/identity/user/login";
const PRECHECK: &str = "https://login.xero.com/identity/user/login/pre-check";

fn field(html: &str, name: &str) -> Option<String> {
    let at = html.find(&format!("name=\"{name}\""))?;
    let rest = &html[at..];
    let start = rest.find("value=\"")?;
    let tail = &rest[start + 7..];
    let end = tail.find('"')?;
    Some(tail[..end].to_string())
}

fn headers(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AkamaiConfig {
        page_url: Some(PAGE.into()),
        wait_ms: Some(100),
        rounds: Some(1),
        ..Default::default()
    };

    let client = Client::open(&config, OpenOptions::default())?;

    let found = client.discover(&DiscoverInput::default())?;
    println!("discover: status {} protected {}", found.status, found.protected);

    let solved = client.solve(&SolveInput::default())?;
    println!(
        "solve: payload {} bytes, posts {}",
        solved.payload.as_deref().map(str::len).unwrap_or(0),
        solved.posts
    );

    let state = client.page()?;
    let html = if state.html.is_empty() {
        client
            .request(&RequestInput {
                url: PAGE.into(),
                ..Default::default()
            })?
            .body
    } else {
        state.html.clone()
    };

    let token = state
        .fields
        .get("__RequestVerificationToken")
        .cloned()
        .or_else(|| field(&html, "__RequestVerificationToken"))
        .ok_or("no antiforgery token")?;

    let return_url = state
        .fields
        .get("ReturnUrl")
        .cloned()
        .or_else(|| field(&html, "ReturnUrl"))
        .unwrap_or_default();

    let username = format!(
        "nx{:x}@example.com",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
    );

    client.request(&RequestInput {
        url: PRECHECK.into(),
        method: Some("POST".into()),
        json: Some(json!({ "Username": username })),
        headers: Some(headers(&[
            ("accept", "application/json, text/plain, */*"),
            ("origin", "https://login.xero.com"),
            ("requestverificationtoken", &token),
        ])),
        ..Default::default()
    })?;

    let mut form = BTreeMap::new();
    form.insert("ReturnUrl".to_string(), return_url);
    form.insert("PreCheckCompleted".to_string(), "true".to_string());
    form.insert("Username".to_string(), username);
    form.insert("Password".to_string(), "Nx7!aQ2zR9kL".to_string());
    form.insert("__RequestVerificationToken".to_string(), token);

    let answer = client.request(&RequestInput {
        url: PAGE.into(),
        method: Some("POST".into()),
        form: Some(form),
        headers: Some(headers(&[
            ("accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
            ("origin", "https://login.xero.com"),
            ("sec-fetch-dest", "document"),
            ("sec-fetch-mode", "navigate"),
            ("sec-fetch-site", "same-origin"),
            ("upgrade-insecure-requests", "1"),
        ])),
        ..Default::default()
    })?;

    let body = answer.body.to_lowercase();
    println!(
        "login: status {} refused {} credential_error {}",
        answer.status,
        answer.refused,
        body.contains("email address or password") || body.contains("incorrect")
    );

    client.close()?;
    Ok(())
}
```

`discover` reports the surface without running the sensor, so it is the cheapest way to tell whether a page is protected. `page` returns the document the session last loaded along with every input it declares, which saves a second fetch. `refused` is true on a 403, a 429, an access denied body or a challenge redirect, so a `false` there with a credential error in the body means the session passed and the login itself was rejected.

`wre-client` in this repository is the same surface without a generated crate, and is covered on the [Rust package page](/web-re-toolkit/packages/rust/). What the client does and what the config controls is in [The Akamai client](/web-re-toolkit/guides/akamai/).
