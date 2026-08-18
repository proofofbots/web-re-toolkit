use serde_json::{Value, json};

use wre_client::client::{Client, prepare, prepare_params};
use wre_client::context::{Call, Ctx, Services};
use wre_client::spec::ClientDescriptor;

const PAGE: &str = "https://login.xero.com/identity/user/login";
const PRECHECK: &str = "https://login.xero.com/identity/user/login/pre-check";

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
            std::env::temp_dir().join("wre-akamai-gate"),
            std::sync::Arc::new(wre_client::context::Counters::default()),
        )
        .expect("services");

        let ctx = Ctx::new("akamai", "gate", services);
        let descriptor = wre_client_akamai::describe().seal().expect("descriptor");
        let config = prepare(&descriptor, &descriptor.config, config, "config").expect("config");
        let client = (wre_client_akamai::registration().build)(ctx, config).expect("client");

        Self { client, descriptor }
    }

    fn call(&mut self, op: &str, params: Value) -> Value {
        let params = prepare_params(&self.descriptor, op, params).expect("params");
        let call = Call::detached(op);

        self.client
            .call(op, params, &call)
            .unwrap_or_else(|error| panic!("{op} failed: {error}"))
    }
}

fn field(html: &str, name: &str) -> Option<String> {
    let anchor = format!("name=\"{name}\"");
    let at = html.find(&anchor)?;
    let rest = &html[at..];
    let value_at = rest.find("value=\"")? + "value=\"".len();
    let tail = &rest[value_at..];
    let end = tail.find('"')?;

    Some(tail[..end].to_string())
}

fn junk_user() -> String {
    format!("nx{:x}@example.com", wre_sandbox::browser::now_ms() as u64)
}

fn login(driver: &mut Driver) -> Value {
    let page = driver.call("page", json!({}));
    let html = page["html"].as_str().unwrap_or_default().to_string();

    let html = if html.is_empty() {
        driver.call("request", json!({ "url": PAGE }))["body"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    } else {
        html
    };

    let token = page["fields"]["__RequestVerificationToken"]
        .as_str()
        .map(str::to_string)
        .or_else(|| field(&html, "__RequestVerificationToken"))
        .expect("antiforgery token");

    let return_url = page["fields"]["ReturnUrl"]
        .as_str()
        .map(str::to_string)
        .or_else(|| field(&html, "ReturnUrl"))
        .unwrap_or_default();

    let username = junk_user();

    driver.call(
        "request",
        json!({
            "url": PRECHECK,
            "method": "POST",
            "json": { "Username": username },
            "headers": {
                "accept": "application/json, text/plain, */*",
                "origin": "https://login.xero.com",
                "requestverificationtoken": token,
            },
        }),
    );

    driver.call(
        "request",
        json!({
            "url": PAGE,
            "method": "POST",
            "form": {
                "ReturnUrl": return_url,
                "PreCheckCompleted": "true",
                "Username": username,
                "Password": "Nx7!aQ2zR9kL",
                "__RequestVerificationToken": token,
            },
            "headers": {
                "accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                "origin": "https://login.xero.com",
                "sec-fetch-dest": "document",
                "sec-fetch-mode": "navigate",
                "sec-fetch-site": "same-origin",
                "upgrade-insecure-requests": "1",
            },
        }),
    )
}

#[test]
#[ignore = "reaches login.xero.com"]
fn the_login_endpoint_serves_a_session_this_client_warmed() {
    let mut driver = Driver::new(json!({ "page_url": PAGE, "wait_ms": 8_000, "rounds": 2 }));

    let found = driver.call("discover", json!({}));
    assert_eq!(found["protected"], true, "xero is not serving a sensor: {found}");

    let solved = driver.call("solve", json!({}));

    let payload = solved["payload"].as_str().expect("payload");
    assert!(payload.len() > 500, "the payload is only {} bytes", payload.len());
    assert!(
        solved["posts"].as_array().unwrap().iter().any(|post| post["status"] == 201),
        "no post was accepted: {}",
        solved["posts"]
    );

    let answer = login(&mut driver);

    assert_eq!(answer["refused"], false, "the login was refused: {}", answer["status"]);
    assert_eq!(answer["status"], 200, "{}", answer["body"]);

    let body = answer["body"].as_str().unwrap_or_default().to_lowercase();
    assert!(
        body.contains("email address or password") || body.contains("incorrect"),
        "the answer does not read like a credential error"
    );
}

#[test]
#[ignore = "reaches login.xero.com"]
fn the_same_login_refuses_a_session_that_never_ran_the_sensor() {
    let mut driver = Driver::new(json!({ "page_url": PAGE }));
    let answer = login(&mut driver);

    assert_eq!(answer["refused"], true, "an unwarmed session was served: {}", answer["status"]);
}

#[test]
#[ignore = "reaches login.xero.com"]
fn a_damaged_payload_is_refused_where_the_real_one_is_served() {
    let mut driver = Driver::new(
        json!({ "page_url": PAGE, "wait_ms": 20_000, "rounds": 0, "live_xhr": false }),
    );

    driver.call("solve", json!({ "post": false }));
    driver.call("post", json!({ "payload": "garbage", "rounds": 2 }));

    let answer = login(&mut driver);

    assert_eq!(answer["refused"], true, "a garbage payload was served: {}", answer["status"]);
}
