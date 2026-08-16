use serde_json::{Value, json};

use wre_client::client::{Client, prepare, prepare_params};
use wre_client::context::{Call, Ctx, Services};
use wre_client::error::ClientError;
use wre_client::spec::ClientDescriptor;

const PAGE: &str = "https://www.realestate.com.au/buy";

struct Driver {
    client: Box<dyn Client>,
    descriptor: ClientDescriptor,
}

impl Driver {
    fn new(config: Value) -> Self {
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .find(|dir| dir.join("wre.toml").is_file() || dir.join(".git").is_dir())
            .map(std::path::Path::to_path_buf);

        let services = Services::new(
            workspace,
            std::env::temp_dir().join("wre-kasada-gate"),
            std::sync::Arc::new(wre_client::context::Counters::default()),
        )
        .expect("services");

        let ctx = Ctx::new("kasada", "gate", services);
        let descriptor = wre_client_kasada::describe().seal().expect("descriptor");
        let config = prepare(&descriptor, &descriptor.config, config, "config").expect("config");
        let client = (wre_client_kasada::registration().build)(ctx, config).expect("client");

        Self { client, descriptor }
    }

    fn call(&mut self, op: &str, params: Value) -> Value {
        self.try_call(op, params).unwrap_or_else(|error| {
            panic!(
                "{op} failed: {error}\n{}",
                serde_json::to_string_pretty(&error.detail).unwrap_or_default()
            )
        })
    }

    fn try_call(&mut self, op: &str, params: Value) -> Result<Value, ClientError> {
        let params = prepare_params(&self.descriptor, op, params).expect("params");
        let call = Call::detached(op);

        self.client.call(op, params, &call)
    }
}

#[test]
#[ignore = "reaches www.realestate.com.au"]
fn the_page_is_served_to_a_session_that_answered_the_interrogation() {
    let mut driver = Driver::new(json!({ "page_url": PAGE, "wait_ms": 25_000 }));

    let found = driver.call("discover", json!({}));
    assert_eq!(
        found["protected"], true,
        "no interrogation was served: {found}"
    );
    assert_eq!(
        found["status"], 429,
        "the page did not answer with a challenge: {found}"
    );

    let solved = driver.call("solve", json!({}));

    assert_eq!(solved["verdict"], "solved", "{solved}");
    assert!(
        solved["token"].as_str().unwrap_or_default().len() > 100,
        "the token is too short: {}",
        solved["token"]
    );
    assert!(
        solved["payload_bytes"].as_u64().unwrap_or_default() > 500,
        "the payload is only {} bytes",
        solved["payload_bytes"]
    );

    assert!(
        solved["clearance"].is_string(),
        "the edge issued no clearance: {solved}"
    );

    let answer = driver.call("request", json!({ "url": PAGE }));

    assert_eq!(
        answer["status"], 200,
        "the page was refused: {}",
        answer["body"]
    );
    assert!(
        answer["bytes"].as_u64().unwrap_or_default() > 100_000,
        "that is the interstitial, not the page: {} bytes",
        answer["bytes"]
    );
}

#[test]
#[ignore = "reaches www.realestate.com.au"]
fn the_same_page_refuses_a_session_that_never_answered() {
    let mut driver = Driver::new(json!({ "page_url": PAGE }));
    let answer = driver.call("request", json!({ "url": PAGE, "token": false }));

    assert_eq!(
        answer["status"], 429,
        "an unsolved session was served the page"
    );
}

#[test]
#[ignore = "reaches www.realestate.com.au"]
fn the_agent_flags_nothing_about_the_sandbox_it_ran_in() {
    let mut driver = Driver::new(json!({ "page_url": PAGE, "wait_ms": 25_000 }));

    driver.call("solve", json!({}));

    let report = driver.call("report", json!({}));
    let flagged = report["flagged"].as_array().cloned().unwrap_or_default();

    assert!(
        flagged.is_empty(),
        "the agent flagged {} checks: {}",
        flagged.len(),
        serde_json::to_string(&flagged).unwrap_or_default()
    );
}
