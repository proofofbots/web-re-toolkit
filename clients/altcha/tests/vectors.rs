use serde_json::{Map, Value, json};

use wre_client_altcha::challenge::{self, Version};
use wre_client_altcha::obfuscation;
use wre_client_altcha::pow::{self, CounterMode, Parameters};
use wre_client_altcha::signature;

const NONCE: &str = "000102030405060708090a0b0c0d0e0f";
const SALT: &str = "aabbccddeeff00112233445566778899";
const SECRET: &str = "signature.secret";

fn parameters(algorithm: &str, cost: u32, key_length: usize) -> Parameters {
    Parameters {
        algorithm: algorithm.to_string(),
        nonce: hex::decode(NONCE).unwrap(),
        salt: hex::decode(SALT).unwrap(),
        cost,
        key_length,
        key_prefix: String::new(),
        memory_cost: None,
        parallelism: None,
    }
}

fn derive(parameters: &Parameters, counter: u64, mode: CounterMode) -> String {
    let password = pow::password(&parameters.nonce, counter, mode);
    hex::encode(pow::derive_key(parameters, &password).unwrap())
}

#[test]
fn sha_matches_the_widget() {
    assert_eq!(
        derive(&parameters(pow::SHA_256, 1, 32), 42, CounterMode::Uint32),
        "9efa368a91e3b314f130aeb951166ef9cb20a1cc5c35817f8c1e43723437f823"
    );
    assert_eq!(
        derive(&parameters(pow::SHA_512, 7, 64), 3, CounterMode::Uint32),
        "2189ba1de0f4cb9672c9a3b5800b64e70778974a5750396f336e70eac17a9ddc\
         54a6d38291053e88b20a9eb486accab7607837311f61ca09ca02e26ddc570bd7"
    );
    assert_eq!(
        derive(&parameters(pow::SHA_256, 3, 16), 5, CounterMode::Uint32),
        "996d1500947654f0f314ce9c2f297129"
    );
}

#[test]
fn pbkdf2_matches_the_widget() {
    assert_eq!(
        derive(&parameters(pow::PBKDF2_SHA_256, 5000, 32), 11, CounterMode::Uint32),
        "83155c14e20e4f11e77e971f5b8d6a88a42a943262b61be4658e37df833d058c"
    );
    assert_eq!(
        derive(&parameters(pow::PBKDF2_SHA_512, 1000, 16), 2, CounterMode::Uint32),
        "66fd5c968999090fe908437f7b3f9383"
    );
}

#[test]
fn scrypt_matches_the_widget() {
    let mut params = parameters(pow::SCRYPT, 1024, 32);
    params.memory_cost = Some(8);
    params.parallelism = Some(1);

    assert_eq!(
        derive(&params, 4, CounterMode::Uint32),
        "1f243938c54c93d41ae23c3f6a1f53392bf8e2723d95c7437b2800bb41392a5c"
    );
}

#[test]
fn argon2id_matches_the_widget() {
    let mut params = parameters(pow::ARGON2ID, 2, 32);
    params.memory_cost = Some(1024);
    params.parallelism = Some(1);

    assert_eq!(
        derive(&params, 9, CounterMode::Uint32),
        "30b539f9ed26ffda3cda0dbe5f927522ec5af27bcb479c82a9af9ddc22768b2c"
    );
}

#[test]
fn canonical_json_signs_like_the_widget() {
    let parameters = json!({
        "keyPrefix": "abababababababababababababababab",
        "algorithm": "SHA-256",
        "salt": SALT,
        "cost": 100,
        "nonce": NONCE,
        "keyLength": 32,
        "expiresAt": 1700000000i64,
    });

    assert_eq!(
        challenge::canonical_json(&parameters),
        "{\"algorithm\":\"SHA-256\",\"cost\":100,\"expiresAt\":1700000000,\"keyLength\":32,\
         \"keyPrefix\":\"abababababababababababababababab\",\
         \"nonce\":\"000102030405060708090a0b0c0d0e0f\",\
         \"salt\":\"aabbccddeeff00112233445566778899\"}"
    );
    assert_eq!(
        challenge::hmac_hex(
            "SHA-256",
            challenge::canonical_json(&parameters).as_bytes(),
            SECRET
        )
        .unwrap(),
        "db78e461a9f9f24da00002946ce05c3d2557f78cfa90ad27302713862658ad34"
    );
}

#[test]
fn a_v3_challenge_solves_to_the_counter_the_server_used() {
    let source = json!({
        "parameters": {
            "algorithm": "SHA-256",
            "cost": 10,
            "keyLength": 32,
            "keyPrefix": "a9a9e81c084ad44c25351c5f5568ce22",
            "nonce": "9295b841727d48460ca4b4954584a14f",
            "salt": "b83df484bc74cdde5caad7992284ca01"
        },
        "signature": "b3aeb49b8a27accf753a38d53e180ce8aaf6cacf72480719f4872d884574a168"
    });

    let parsed = challenge::parse(&source).unwrap();
    assert_eq!(parsed.version, Version::V3);

    let solved = pow::solve(&parsed.parameters, parsed.counter_mode(), 0, 10_000, 4, &|| false)
        .unwrap()
        .expect("the challenge has a solution");

    assert_eq!(solved.counter, 37);
    assert_eq!(
        hex::encode(&solved.derived_key),
        "a9a9e81c084ad44c25351c5f5568ce22229214aa726b13edb33baa9675fee5b3"
    );

    let signature = challenge::hmac_hex(
        "SHA-256",
        challenge::canonical_json(&parsed.raw_parameters).as_bytes(),
        SECRET,
    )
    .unwrap();
    assert_eq!(Some(signature), parsed.signature);

    let payload = parsed.payload(solved.counter, &solved.derived_key, 12.3);
    assert_eq!(payload["solution"]["counter"], json!(37));
    assert_eq!(payload["challenge"]["parameters"]["nonce"], json!("9295b841727d48460ca4b4954584a14f"));
}

#[test]
fn a_v1_challenge_keeps_the_legacy_shape() {
    let source = json!({
        "algorithm": "SHA-256",
        "challenge": "f4cb24d4a5bffabadd37c4ce3cfb5246adc8f2cb88e3e49497f0b3c3af90ffaf",
        "salt": "saltysalt?expires=1700000000",
        "signature": "c053bcd07083810a8a9412efb35168a56b6a2d8ca03bbe9e52ceceb69f591d9a"
    });

    let parsed = challenge::parse(&source).unwrap();
    assert_eq!(parsed.version, Version::V1);
    assert_eq!(parsed.expires_at, Some(1700000000));
    assert_eq!(parsed.counter_mode(), CounterMode::Text);

    let derived = derive(&parsed.parameters, 12345, CounterMode::Text);
    assert_eq!(derived, parsed.parameters.key_prefix);

    assert_eq!(
        challenge::hmac_hex("SHA-256", derived.as_bytes(), SECRET).unwrap(),
        parsed.signature.clone().unwrap()
    );

    let payload = parsed.payload(12345, &hex::decode(&derived).unwrap(), 4.2);
    assert_eq!(payload["number"], json!(12345));
    assert_eq!(payload["salt"], json!("saltysalt?expires=1700000000"));
    assert_eq!(payload["challenge"], json!(parsed.parameters.key_prefix));
}

#[test]
fn the_obfuscation_plugin_round_trips() {
    let data = "eyJwYXJhbWV0ZXJzIjp7ImFsZ29yaXRobSI6IlBCS0RGMi9TSEEtMjU2IiwiY29zdCI6MjAwLCJrZXlMZW5ndGgiOjMyLCJrZXlQcmVmaXgiOiI5YzEyNDEyM2VlZTM5ZmU2ZmE0N2ZjMGY1NDk5NWQyNiIsIm5vbmNlIjoiOTUwNWNkNzljMjFkMmEwMzIwZWJiN2ZmMGEzZmM3ZmYiLCJzYWx0IjoiNmNiMDllMGUyZjc1MWUxYTMwOWY3MzZiODFjZmM1YjAifSwiY2lwaGVyIjp7Iml2IjoiOTk4MTBlYmFiZmRlNGE1ZTg2NjYxN2M1IiwiZGF0YSI6Ijc3ZWE0MDg2OTlmODU1ZDYwYzQwOGFkNzQ4NWI5NWVjMmZkMDJjNWJmMzg4NDc2YTI0ODgzMDgzZmY0M2Q4ODM5M2JkMjJkMzE3Y2MwYTI5MGMifX0=";

    let parsed = obfuscation::parse(data).unwrap();
    let solved = pow::solve(
        &parsed.parameters,
        obfuscation::COUNTER_MODE,
        0,
        1_000,
        4,
        &|| false,
    )
    .unwrap()
    .expect("the obfuscated data carries a solvable challenge");

    let text = obfuscation::decrypt(&solved.derived_key, &parsed.iv, &parsed.data).unwrap();
    assert_eq!(text, "mailto:hidden@example.com");
}

#[test]
fn a_server_signature_verifies() {
    let payload = "eyJhbGdvcml0aG0iOiJTSEEtMjU2IiwidmVyaWZpY2F0aW9uRGF0YSI6ImNsYXNzaWZpY2F0aW9uPUdPT0QmZW1haWw9dXNlciU0MGV4YW1wbGUuY29tJmV4cGlyZT00MTAyNDQ0ODAwJmZpZWxkcz1lbWFpbCUyQ25hbWUmZmllbGRzSGFzaD1mMmM3MzUyNDAwMDM5ZmQ5MzBiNTlkMzY5ZTgxZTEzMjlhMjg0ZDJmM2M0YzI1OTEwZWQ2YTgyMDhiM2Q1MTEyJnNjb3JlPTEuNCZ0aW1lPTE3MDAwMDAwMDAmdmVyaWZpZWQ9dHJ1ZSIsInNpZ25hdHVyZSI6ImE4MzYwZTkzMTE5OWZhYmNkZjQ3ODhjMDc3YzY5ZmNjMjQ0YTIzOGFiMzk3MDgwY2QzNTMyZWI5ZDQyODk2NGYiLCJ2ZXJpZmllZCI6dHJ1ZX0=";
    let decoded = wre_client::shape::decode_bytes(payload).unwrap();
    let payload: Value = serde_json::from_slice(&decoded).unwrap();

    let mut form = Map::new();
    form.insert("email".to_string(), json!("user@example.com"));
    form.insert("name".to_string(), json!("Ada"));

    let checked = signature::verify(&payload, SECRET, 1_700_000_100, Some(&form)).unwrap();

    assert_eq!(checked["verified"], json!(true));
    assert_eq!(checked["expired"], json!(false));
    assert_eq!(checked["invalidSignature"], json!(false));
    assert_eq!(checked["fieldsValid"], json!(true));
    assert_eq!(checked["verificationData"]["classification"], json!("GOOD"));
    assert_eq!(checked["verificationData"]["score"], json!(1.4));
    assert_eq!(checked["verificationData"]["fields"], json!(["email", "name"]));
    assert_eq!(checked["verificationData"]["email"], json!("user@example.com"));

    let expired = signature::verify(&payload, SECRET, 4_200_000_000, None).unwrap();
    assert_eq!(expired["expired"], json!(true));
    assert_eq!(expired["verified"], json!(false));
}
