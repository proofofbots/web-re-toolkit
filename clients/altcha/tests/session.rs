use serde_json::{Value, json};

use wre_client::client::Client;
use wre_client::context::{Call, Ctx, Services};
use wre_client_altcha::{ID, registration};

fn open(config: Value) -> Box<dyn Client> {
    let services = Services::detached().unwrap();
    let ctx = Ctx::new(ID, "test", services);
    (registration().build)(ctx, config).unwrap()
}

fn call(client: &mut dyn Client, op: &str, params: Value) -> Value {
    client.call(op, params, &Call::detached(op)).unwrap()
}

#[test]
fn a_created_challenge_solves_and_verifies() {
    let mut client = open(json!({
        "hmac_secret": "signature.secret",
        "workers": 4,
        "max_counter": 5000,
        "clock_ms": 1_700_000_000_000u64,
        "seed": 7,
    }));

    let created = call(
        client.as_mut(),
        "create_challenge",
        json!({ "algorithm": "SHA-256", "cost": 50, "counter": 411, "expires_in_s": 600 }),
    );

    let challenge = created["challenge"].clone();
    let solved = call(client.as_mut(), "solve", json!({ "challenge": challenge }));
    assert_eq!(solved["counter"], json!(411));
    assert_eq!(solved["field"], json!("altcha"));

    let checked = call(
        client.as_mut(),
        "verify",
        json!({ "payload": solved["payload"] }),
    );
    assert_eq!(checked["verified"], json!(true));
    assert_eq!(checked["format"], json!(3));
}

#[test]
fn a_created_v1_challenge_solves_and_verifies() {
    let mut client = open(json!({
        "hmac_secret": "signature.secret",
        "workers": 4,
        "max_counter": 5000,
        "clock_ms": 1_700_000_000_000u64,
    }));

    let created = call(
        client.as_mut(),
        "create_challenge",
        json!({ "format": 1, "counter": 900, "expires_in_s": 600 }),
    );

    let solved = call(
        client.as_mut(),
        "solve",
        json!({ "challenge": created["challenge"].clone() }),
    );
    assert_eq!(solved["counter"], json!(900));
    assert_eq!(solved["format"], json!(1));

    let checked = call(
        client.as_mut(),
        "verify",
        json!({ "payload": solved["payload"] }),
    );
    assert_eq!(checked["verified"], json!(true));
    assert_eq!(checked["format"], json!(1));
}

#[test]
fn an_expired_challenge_is_reported() {
    let mut client = open(json!({
        "hmac_secret": "signature.secret",
        "workers": 2,
        "max_counter": 2000,
        "clock_ms": 1_700_000_000_000u64,
    }));

    let created = call(
        client.as_mut(),
        "create_challenge",
        json!({ "cost": 10, "counter": 7, "expires_in_s": -60 }),
    );
    let solved = call(
        client.as_mut(),
        "solve",
        json!({ "challenge": created["challenge"].clone() }),
    );
    let checked = call(
        client.as_mut(),
        "verify",
        json!({ "payload": solved["payload"] }),
    );

    assert_eq!(checked["expired"], json!(true));
    assert_eq!(checked["verified"], json!(false));
}

#[test]
fn interaction_samples_are_reproducible_under_a_seed() {
    let config = json!({ "seed": 42, "clock_ms": 1_700_000_000_000u64 });
    let mut first = open(config.clone());
    let mut second = open(config);

    let params = json!({ "width": 1440, "height": 900, "duration_ms": 1200, "scroll": true });
    let left = call(first.as_mut(), "his", params.clone());
    let right = call(second.as_mut(), "his", params);

    assert_eq!(left, right);
    assert_eq!(left["time"], json!(1_700_000_000_000u64));
    assert!(left["pointer"].as_array().unwrap().len() >= 6);
    assert!(left["touch"].as_array().unwrap().is_empty());

    let touch = call(first.as_mut(), "his", json!({ "touch": true }));
    assert_eq!(touch["maxTouchPoints"], json!(5));
    assert!(touch["pointer"].as_array().unwrap().is_empty());
}
