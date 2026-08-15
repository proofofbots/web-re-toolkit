use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use serde_json::Value;

use crate::challenge;
use crate::pow::{CounterMode, Parameters};

pub struct Obfuscated {
    pub parameters: Parameters,
    pub iv: Vec<u8>,
    pub data: Vec<u8>,
}

pub const COUNTER_MODE: CounterMode = CounterMode::Uint32;

pub fn parse(encoded: &str) -> Result<Obfuscated, String> {
    let decoded = wre_client::shape::decode_bytes(encoded)
        .map_err(|error| format!("obfuscated data is not base64: {error}"))?;
    let value: Value = serde_json::from_slice(&decoded)
        .map_err(|error| format!("obfuscated data is not json: {error}"))?;

    let cipher = value
        .get("cipher")
        .ok_or_else(|| "obfuscated data has no cipher".to_string())?;
    let iv = cipher
        .get("iv")
        .and_then(Value::as_str)
        .ok_or_else(|| "cipher has no iv".to_string())?;
    let data = cipher
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| "cipher has no data".to_string())?;

    let parsed = challenge::parse(&value)?;

    Ok(Obfuscated {
        parameters: parsed.parameters,
        iv: hex::decode(iv).map_err(|error| format!("iv is not hex: {error}"))?,
        data: hex::decode(data).map_err(|error| format!("cipher data is not hex: {error}"))?,
    })
}

pub fn decrypt(key: &[u8], iv: &[u8], data: &[u8]) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|error| format!("aes key rejected: {error}"))?;
    let nonce = Nonce::try_from(iv).map_err(|_| format!("iv must be 12 bytes, got {}", iv.len()))?;
    let clear = cipher
        .decrypt(&nonce, data)
        .map_err(|_| "aes-gcm decryption failed, the derived key does not open this cipher".to_string())?;

    String::from_utf8(clear).map_err(|error| format!("cleartext is not utf-8: {error}"))
}
