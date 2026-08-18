
use std::time::Duration;

use wre_net::emulate::Fingerprint;
use wre_net::http::{Client, ClientOptions};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let url = args.next().unwrap_or_else(|| "https://localhost:9443/".to_string());
    let spec = args.next();

    let fingerprint = match spec.as_deref() {
        Some(text) => match text.parse::<Fingerprint>() {
            Ok(found) => Some(found),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        },
        None => None,
    };

    let options = ClientOptions {
        fingerprint,
        accept_invalid_certs: true,
        http2_only: true,
        timeout: Duration::from_secs(10),
        ..ClientOptions::default()
    };

    let resolved = options.resolved_fingerprint();
    let client = match Client::new(options) {
        Ok(found) => found,
        Err(error) => {
            eprintln!("client: {error}");
            std::process::exit(1);
        }
    };

    println!("emulating {resolved}");
    println!("user agent {}", client.user_agent().unwrap_or_default());

    match client.get_text(&url).await {
        Ok(body) => println!("answered {} bytes", body.len()),
        Err(error) => println!("no answer: {error}"),
    }
}
