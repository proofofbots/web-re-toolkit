use serde_json::{Map, Value, json};

use crate::challenge::{constant_time_equal, decode_component, digest, hmac_hex};

const ARRAY_FIELDS: [&str; 2] = ["fields", "reasons"];

pub fn parse_verification_data(data: &str) -> Map<String, Value> {
    let mut out = Map::new();

    for pair in data.split('&').filter(|part| !part.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let name = decode_component(name);
        let value = decode_component(value);
        out.insert(name.clone(), typed(&name, value.trim()));
    }

    out
}

pub fn verify(
    payload: &Value,
    secret: &str,
    now_seconds: i64,
    form: Option<&Map<String, Value>>,
) -> Result<Value, String> {
    let algorithm = payload
        .get("algorithm")
        .and_then(Value::as_str)
        .unwrap_or("SHA-256");
    let data = payload
        .get("verificationData")
        .and_then(Value::as_str)
        .ok_or_else(|| "payload has no verificationData".to_string())?;
    let signature = payload
        .get("signature")
        .and_then(Value::as_str)
        .ok_or_else(|| "payload has no signature".to_string())?;

    let expected = hmac_hex(algorithm, &digest(algorithm, data.as_bytes()), secret)?;
    let verification_data = parse_verification_data(data);

    let expired = verification_data
        .get("expire")
        .and_then(Value::as_i64)
        .is_some_and(|expire| expire < now_seconds);
    let invalid_signature = !constant_time_equal(signature, &expected);
    let invalid_solution = verification_data.get("verified").and_then(Value::as_bool) != Some(true)
        || payload.get("verified").and_then(Value::as_bool) != Some(true);

    let fields_valid = match (form, verification_data.get("fieldsHash")) {
        (Some(form), Some(Value::String(expected))) => {
            let fields: Vec<String> = verification_data
                .get("fields")
                .and_then(Value::as_array)
                .map(|names| {
                    names
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            Some(&fields_hash(algorithm, &fields, form) == expected)
        }
        _ => None,
    };

    Ok(json!({
        "expired": expired,
        "invalidSignature": invalid_signature,
        "invalidSolution": invalid_solution,
        "fieldsValid": fields_valid,
        "verificationData": Value::Object(verification_data),
        "verified": !expired && !invalid_signature && !invalid_solution && fields_valid != Some(false),
    }))
}

pub fn fields_hash(algorithm: &str, fields: &[String], form: &Map<String, Value>) -> String {
    let lines: Vec<String> = fields
        .iter()
        .map(|name| match form.get(name) {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        })
        .collect();

    hex::encode(digest(algorithm, lines.join("\n").as_bytes()))
}

fn typed(name: &str, value: &str) -> Value {
    if value == "true" || value == "false" {
        return json!(value == "true");
    }
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        if let Ok(number) = value.parse::<i64>() {
            return json!(number);
        }
    }
    if let Some((whole, fraction)) = value.split_once('.') {
        let numeric = !whole.is_empty()
            && !fraction.is_empty()
            && whole.bytes().all(|byte| byte.is_ascii_digit())
            && fraction.bytes().all(|byte| byte.is_ascii_digit());
        if numeric {
            if let Ok(number) = value.parse::<f64>() {
                return json!(number);
            }
        }
    }
    if ARRAY_FIELDS.contains(&name) {
        return json!(value.split(',').collect::<Vec<&str>>());
    }
    json!(value)
}
