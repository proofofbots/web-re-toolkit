use serde_json::{Value, json};

use wre_client::client::Client;
use wre_client::context::{Call, Ctx, Services};
use wre_client_altcha::{ID, registration};

const LAB: &str = "http://localhost:8791";

fn call(client: &mut dyn Client, op: &str, params: Value) -> Value {
    client.call(op, params, &Call::detached(op)).unwrap()
}

#[test]
#[ignore = "needs reference/altcha/lab/server.py running on port 8791"]
fn the_lab_server_accepts_a_headless_solve() {
    let services = Services::detached().unwrap();
    let ctx = Ctx::new(ID, "lab", services);
    let mut client = (registration().build)(
        ctx,
        json!({
            "challenge_url": format!("{LAB}/altcha/challenge"),
            "verify_url": format!("{LAB}/verify"),
            "max_counter": 300_000,
        }),
    )
    .unwrap();

    let solved = call(client.as_mut(), "solve", json!({}));
    assert_eq!(solved["format"], json!(3));
    assert!(solved["counter"].as_u64().unwrap() > 0);

    let submitted = call(
        client.as_mut(),
        "submit",
        json!({ "payload": solved["payload"], "fields": { "email": "ada@example.com" } }),
    );

    assert_eq!(submitted["status"], json!(200));
    assert_eq!(submitted["body"]["verified"], json!(true));
    assert_eq!(submitted["body"]["counter"], solved["counter"]);
}
