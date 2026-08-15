use std::collections::BTreeMap;

use hmac::digest::KeyInit;
use hmac::{Hmac, Mac};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256, Sha384, Sha512};

use crate::pow::{CounterMode, Parameters};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V1,
    V3,
}

#[derive(Debug, Clone)]
pub struct Challenge {
    pub version: Version,
    pub parameters: Parameters,
    pub raw_parameters: Value,
    pub signature: Option<String>,
    pub original_salt: Option<String>,
    pub expires_at: Option<i64>,
}

impl Challenge {
    pub fn counter_mode(&self) -> CounterMode {
        match self.version {
            Version::V1 => CounterMode::Text,
            Version::V3 => CounterMode::Uint32,
        }
    }

    pub fn payload(&self, counter: u64, derived_key: &[u8], took_ms: f64) -> Value {
        match self.version {
            Version::V1 => json!({
                "algorithm": self.parameters.algorithm,
                "challenge": self.parameters.key_prefix,
                "number": counter,
                "salt": self
                    .original_salt
                    .clone()
                    .unwrap_or_else(|| hex::encode(&self.parameters.nonce)),
                "signature": self.signature,
                "took": took_ms,
            }),
            Version::V3 => json!({
                "challenge": {
                    "parameters": self.raw_parameters,
                    "signature": self.signature,
                },
                "solution": {
                    "counter": counter,
                    "derivedKey": hex::encode(derived_key),
                    "time": took_ms,
                },
            }),
        }
    }
}

pub fn parse(source: &Value) -> Result<Challenge, String> {
    let object = source
        .as_object()
        .ok_or_else(|| "challenge must be a json object".to_string())?;

    if object.contains_key("challenge") && !object.contains_key("parameters") {
        parse_v1(object)
    } else {
        parse_v3(object)
    }
}

fn parse_v1(object: &Map<String, Value>) -> Result<Challenge, String> {
    let algorithm = text(object, "algorithm").unwrap_or_else(|| "SHA-256".to_string());
    let key_prefix = text(object, "challenge")
        .ok_or_else(|| "v1 challenge is missing challenge".to_string())?;
    let salt = text(object, "salt").ok_or_else(|| "v1 challenge is missing salt".to_string())?;

    let mut data = Map::new();
    if let Some((_, query)) = salt.split_once('?') {
        for pair in query.split('&').filter(|part| !part.is_empty()) {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            data.insert(decode_component(name), json!(decode_component(value)));
        }
    }

    let expires_at = data
        .get("expires")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<i64>().ok());

    let key_length = match algorithm.as_str() {
        "SHA-512" => 64,
        "SHA-384" => 48,
        _ => 32,
    };

    let parameters = Parameters {
        algorithm: algorithm.clone(),
        nonce: salt.as_bytes().to_vec(),
        salt: Vec::new(),
        cost: 1,
        key_length,
        key_prefix: key_prefix.clone(),
        memory_cost: None,
        parallelism: None,
    };

    let raw_parameters = json!({
        "algorithm": algorithm,
        "cost": 1,
        "data": Value::Object(data.clone()),
        "keyLength": key_length,
        "keyPrefix": key_prefix,
        "nonce": hex::encode(salt.as_bytes()),
        "salt": "",
    });

    Ok(Challenge {
        version: Version::V1,
        parameters,
        raw_parameters,
        signature: text(object, "signature"),
        original_salt: Some(salt),
        expires_at,
    })
}

fn parse_v3(object: &Map<String, Value>) -> Result<Challenge, String> {
    let raw_parameters = object
        .get("parameters")
        .cloned()
        .ok_or_else(|| "challenge is missing parameters".to_string())?;
    let entries = raw_parameters
        .as_object()
        .ok_or_else(|| "parameters must be a json object".to_string())?;

    let algorithm =
        text(entries, "algorithm").ok_or_else(|| "parameters are missing algorithm".to_string())?;
    let nonce = text(entries, "nonce").ok_or_else(|| "parameters are missing nonce".to_string())?;
    let salt = text(entries, "salt").unwrap_or_default();
    let key_prefix =
        text(entries, "keyPrefix").ok_or_else(|| "parameters are missing keyPrefix".to_string())?;

    let parameters = Parameters {
        algorithm,
        nonce: hex::decode(&nonce).map_err(|error| format!("nonce is not hex: {error}"))?,
        salt: hex::decode(&salt).map_err(|error| format!("salt is not hex: {error}"))?,
        cost: number(entries, "cost").unwrap_or(1.0) as u32,
        key_length: number(entries, "keyLength").unwrap_or(32.0) as usize,
        key_prefix,
        memory_cost: number(entries, "memoryCost").map(|value| value as u32),
        parallelism: number(entries, "parallelism").map(|value| value as u32),
    };

    Ok(Challenge {
        version: Version::V3,
        parameters,
        raw_parameters: raw_parameters.clone(),
        signature: text(object, "signature"),
        original_salt: None,
        expires_at: number(entries, "expiresAt").map(|value| value as i64),
    })
}

pub fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(entries) => {
            let sorted: BTreeMap<&String, &Value> = entries.iter().collect();
            let body: Vec<String> = sorted
                .into_iter()
                .map(|(key, value)| {
                    format!("{}:{}", Value::String(key.clone()), canonical_json(value))
                })
                .collect();
            format!("{{{}}}", body.join(","))
        }
        Value::Array(_) => value.to_string(),
        other => other.to_string(),
    }
}

pub fn hmac_hex(algorithm: &str, data: &[u8], secret: &str) -> Result<String, String> {
    match algorithm {
        "SHA-384" => Ok(hex::encode(sign::<Sha384>(data, secret)?)),
        "SHA-512" => Ok(hex::encode(sign::<Sha512>(data, secret)?)),
        _ => Ok(hex::encode(sign::<Sha256>(data, secret)?)),
    }
}

pub fn digest(algorithm: &str, data: &[u8]) -> Vec<u8> {
    match algorithm {
        "SHA-384" => Sha384::digest(data).to_vec(),
        "SHA-512" => Sha512::digest(data).to_vec(),
        _ => Sha256::digest(data).to_vec(),
    }
}

pub fn constant_time_equal(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut result = 0u8;
    for (a, b) in left.bytes().zip(right.bytes()) {
        result |= a ^ b;
    }
    result == 0
}

fn sign<D>(data: &[u8], secret: &str) -> Result<Vec<u8>, String>
where
    D: Digest + hmac::EagerHash,
    Hmac<D>: Mac,
{
    let mut mac = <Hmac<D> as KeyInit>::new_from_slice(secret.as_bytes())
        .map_err(|error| format!("hmac key rejected: {error}"))?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn text(object: &Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_string)
}

fn number(object: &Map<String, Value>, key: &str) -> Option<f64> {
    object.get(key).and_then(Value::as_f64)
}

pub fn decode_component(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 3 <= bytes.len() => {
                let pair = &text[index + 1..index + 3];
                match u8::from_str_radix(pair, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8_lossy(&out).into_owned()
}
