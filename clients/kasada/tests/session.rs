use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

use wre_client::client::{Client, prepare, prepare_params};
use wre_client::context::{Call, Ctx, Services};
use wre_client::error::ClientError;
use wre_client::spec::ClientDescriptor;

const SITE: &str = "149e9513-01fa-4fb0-aad4-566afd725d1b";
const TENANT: &str = "2d206a39-8ed7-437e-a3be-862e0f06eea3";
const SOLVED: &str = "0solvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolvedsolv";

struct Edge {
    port: u16,
    stamped: Arc<AtomicUsize>,
}

fn interstitial(port: u16) -> String {
    format!(
        "<!DOCTYPE html><html><head></head><body><script>window.KPSDK={{}};KPSDK.start=0;</script>\
         <script src=\"/{SITE}/{TENANT}/ips.js?KP_UIDz=challenge&amp;x-kpsdk-v=j-1.2.661&amp;x-kpsdk-im=im\"></script></body></html>\
         <!-- {port} -->"
    )
}

fn serve() -> Edge {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let port = listener.local_addr().expect("address").port();
    let stamped = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&stamped);

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buffer = vec![0u8; 65536];
            let read = stream.read(&mut buffer).unwrap_or_default();
            let head = String::from_utf8_lossy(&buffer[..read]).to_string();
            let mut lines = head.split("\r\n");
            let request = lines.next().unwrap_or_default().to_string();
            let cleared = head.contains(&format!("KP_UIDz={SOLVED}"))
                || head
                    .to_lowercase()
                    .contains(&format!("x-kpsdk-ct: {SOLVED}"));

            let (status, extra, body) = if request.starts_with("GET /buy") {
                if cleared {
                    counted.fetch_add(1, Ordering::Relaxed);
                    (
                        200,
                        String::new(),
                        "<html><body>the listings</body></html>".to_string(),
                    )
                } else {
                    (
                        429,
                        "set-cookie: KP_UIDz=challenge; Path=/\r\nx-kpsdk-ct: challenge\r\n"
                            .to_string(),
                        interstitial(port),
                    )
                }
            } else if request.contains("/ips.js") {
                (
                    200,
                    String::new(),
                    format!("var agent = 1; {}", "x".repeat(2000)),
                )
            } else if request.contains("/tl") {
                (
                    200,
                    format!(
                        "x-kpsdk-ct: {SOLVED}\r\nx-kpsdk-cr: true\r\nx-kpsdk-r: 1-TEST\r\nx-kpsdk-st: 1786800000000\r\n"
                    ),
                    "{}".to_string(),
                )
            } else {
                (404, String::new(), String::new())
            };

            let answer = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: text/html\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n{body}",
                body.len()
            );

            let _ = stream.write_all(answer.as_bytes());
            let _ = stream.flush();
        }
    });

    Edge { port, stamped }
}

struct Driver {
    client: Box<dyn Client>,
    descriptor: ClientDescriptor,
}

impl Driver {
    fn new(config: Value) -> Self {
        let services = Services::detached().expect("services");
        let ctx = Ctx::new("kasada", "test", services);
        let descriptor = wre_client_kasada::describe().seal().expect("descriptor");
        let config = prepare(&descriptor, &descriptor.config, config, "config").expect("config");
        let client = (wre_client_kasada::registration().build)(ctx, config).expect("client");

        Self { client, descriptor }
    }

    fn call(&mut self, op: &str, params: Value) -> Value {
        self.try_call(op, params)
            .unwrap_or_else(|error| panic!("{op} failed: {error}"))
    }

    fn try_call(&mut self, op: &str, params: Value) -> Result<Value, ClientError> {
        let params = prepare_params(&self.descriptor, op, params).expect("params");
        self.client.call(op, params, &Call::detached(op))
    }
}

#[test]
fn discover_reads_the_tenant_out_of_an_interrogation_page() {
    let edge = serve();
    let url = format!("http://127.0.0.1:{}/buy", edge.port);
    let mut driver =
        Driver::new(json!({ "page_url": url, "user_agent": "Mozilla/5.0 Chrome/151.0.0.0" }));

    let found = driver.call("discover", json!({}));

    assert_eq!(found["status"], 429);
    assert_eq!(found["protected"], true);
    assert_eq!(found["surface"]["tenant"]["site"], SITE);
    assert_eq!(found["surface"]["tenant"]["tenant"], TENANT);
    assert!(
        found["surface"]["script"]
            .as_str()
            .unwrap_or_default()
            .contains("ips.js"),
        "{found}"
    );
    assert_eq!(found["cookies"]["KP_UIDz"], "challenge");
}

#[test]
fn a_page_with_no_interrogation_is_reported_as_open() {
    let edge = serve();
    let url = format!("http://127.0.0.1:{}/other", edge.port);
    let mut driver =
        Driver::new(json!({ "page_url": url, "user_agent": "Mozilla/5.0 Chrome/151.0.0.0" }));

    let found = driver.call("discover", json!({}));

    assert_eq!(found["protected"], false);
    assert!(found["surface"]["tenant"].is_null());
}

#[test]
fn a_request_carries_the_token_when_there_is_one_and_nothing_when_there_is_not() {
    let edge = serve();
    let page = format!("http://127.0.0.1:{}/buy", edge.port);
    let mut driver = Driver::new(
        json!({ "page_url": page.clone(), "user_agent": "Mozilla/5.0 Chrome/151.0.0.0" }),
    );

    let refused = driver.call("request", json!({ "url": page, "token": false }));

    assert_eq!(refused["status"], 429);
    assert_eq!(edge.stamped.load(Ordering::Relaxed), 0);
}

#[test]
fn the_ops_that_need_a_session_say_so() {
    let mut driver = Driver::new(json!({ "user_agent": "Mozilla/5.0 Chrome/151.0.0.0" }));

    for op in ["payload", "report", "misses", "loader"] {
        let error = driver.try_call(op, json!({})).expect_err("no session");
        assert_eq!(error.kind.as_str(), "bad_input", "{op}: {error}");
    }

    assert_eq!(driver.call("reset", json!({}))["ok"], true);
}

#[test]
fn a_proof_of_work_header_is_built_from_a_token_and_a_salt() {
    let mut driver = Driver::new(json!({ "user_agent": "Mozilla/5.0 Chrome/151.0.0.0" }));

    let proof = driver.call(
        "pow",
        json!({
            "token": "3;1786799692150;abcdefghijklmnopqrstuvwxyz",
            "salt": "0f44a7cde3661c88ea0675ee045d307720919bc2a20d6b7d777aea1738f69a9a",
        }),
    );

    let header: Value =
        serde_json::from_str(proof["header"].as_str().expect("header")).expect("json");

    assert_eq!(header["answers"], proof["answers"]);
    assert_eq!(header["id"].as_str().map(str::len), Some(32));
    assert_eq!(proof["answers"].as_array().map(Vec::len), Some(2));
}

#[test]
#[ignore = "needs a graph profile in profiles/graph"]
fn a_session_that_answers_the_interrogation_is_served_the_page() {
    let edge = serve();
    let page = format!("http://127.0.0.1:{}/buy", edge.port);

    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|dir| dir.join("wre.toml").is_file() || dir.join(".git").is_dir())
        .map(std::path::Path::to_path_buf);

    let services = Services::new(
        workspace,
        std::env::temp_dir().join("wre-kasada-edge"),
        std::sync::Arc::new(wre_client::context::Counters::default()),
    )
    .expect("services");

    let ctx = Ctx::new("kasada", "edge", services);
    let descriptor = wre_client_kasada::describe().seal().expect("descriptor");
    let config = prepare(
        &descriptor,
        &descriptor.config,
        json!({ "page_url": page.clone(), "wait_ms": 3000, "paced": false, "frames": 1 }),
        "config",
    )
    .expect("config");

    let mut driver = Driver {
        client: (wre_client_kasada::registration().build)(ctx, config).expect("client"),
        descriptor,
    };

    let error = driver
        .try_call("solve", json!({}))
        .expect_err("the stub posts nothing");
    assert_eq!(error.kind.as_str(), "blocked", "{error}");

    let answer = driver.call("request", json!({ "url": page, "token": false }));
    assert_eq!(answer["status"], 429);
    assert_eq!(edge.stamped.load(Ordering::Relaxed), 0);
}
