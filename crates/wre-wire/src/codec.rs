use std::io::{Read, Write};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use wre_core::digest::sha256;
use wre_core::error::{Error, Result};

pub trait Codec {
    fn name(&self) -> &str;

    fn open(&mut self, bytes: &[u8]) -> Result<Value>;

    fn seal(&mut self, value: &Value) -> Result<Vec<u8>>;
}

#[derive(Debug, Clone, Default)]
pub struct JsonCodec;

impl Codec for JsonCodec {
    fn name(&self) -> &str {
        "json"
    }

    fn open(&mut self, bytes: &[u8]) -> Result<Value> {
        serde_json::from_slice(bytes)
            .map_err(|error| Error::msg(format!("json body did not parse: {error}")))
    }

    fn seal(&mut self, value: &Value) -> Result<Vec<u8>> {
        serde_json::to_vec(value)
            .map_err(|error| Error::msg(format!("value did not serialise: {error}")))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Base64JsonCodec;

impl Codec for Base64JsonCodec {
    fn name(&self) -> &str {
        "base64+json"
    }

    fn open(&mut self, bytes: &[u8]) -> Result<Value> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| Error::msg("base64 body is not text"))?
            .trim();

        let decoded = STANDARD
            .decode(text)
            .map_err(|error| Error::msg(format!("base64 body did not decode: {error}")))?;

        serde_json::from_slice(&decoded)
            .map_err(|error| Error::msg(format!("decoded body is not json: {error}")))
    }

    fn seal(&mut self, value: &Value) -> Result<Vec<u8>> {
        let json = serde_json::to_vec(value)
            .map_err(|error| Error::msg(format!("value did not serialise: {error}")))?;
        Ok(STANDARD.encode(json).into_bytes())
    }
}

#[derive(Debug, Clone, Default)]
pub struct DeflateJsonCodec {
    pub raw: bool,
}

impl DeflateJsonCodec {
    pub fn raw() -> Self {
        Self { raw: true }
    }

    pub fn zlib() -> Self {
        Self { raw: false }
    }
}

impl Codec for DeflateJsonCodec {
    fn name(&self) -> &str {
        if self.raw { "deflate-raw+json" } else { "deflate+json" }
    }

    fn open(&mut self, bytes: &[u8]) -> Result<Value> {
        let mut out = Vec::new();

        if self.raw {
            let mut decoder = flate2::read::DeflateDecoder::new(bytes);
            decoder
                .read_to_end(&mut out)
                .map_err(|error| Error::msg(format!("deflate-raw body did not inflate: {error}")))?;
        } else {
            let mut decoder = flate2::read::ZlibDecoder::new(bytes);
            decoder
                .read_to_end(&mut out)
                .map_err(|error| Error::msg(format!("deflate body did not inflate: {error}")))?;
        }

        serde_json::from_slice(&out)
            .map_err(|error| Error::msg(format!("inflated body is not json: {error}")))
    }

    fn seal(&mut self, value: &Value) -> Result<Vec<u8>> {
        let json = serde_json::to_vec(value)
            .map_err(|error| Error::msg(format!("value did not serialise: {error}")))?;

        let mut out = Vec::new();

        if self.raw {
            let mut encoder =
                flate2::write::DeflateEncoder::new(&mut out, flate2::Compression::default());
            encoder
                .write_all(&json)
                .map_err(|error| Error::msg(format!("deflate-raw failed: {error}")))?;
            encoder
                .finish()
                .map_err(|error| Error::msg(format!("deflate-raw failed: {error}")))?;
        } else {
            let mut encoder =
                flate2::write::ZlibEncoder::new(&mut out, flate2::Compression::default());
            encoder
                .write_all(&json)
                .map_err(|error| Error::msg(format!("deflate failed: {error}")))?;
            encoder
                .finish()
                .map_err(|error| Error::msg(format!("deflate failed: {error}")))?;
        }

        Ok(out)
    }
}

#[derive(Debug, Clone)]
pub struct XorCodec {
    pub key: Vec<u8>,
    pub inner: XorInner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XorInner {
    Json,
    Base64Json,
}

impl XorCodec {
    pub fn new(key: Vec<u8>, inner: XorInner) -> Result<Self> {
        if key.is_empty() {
            return Err(Error::msg("xor key is empty"));
        }
        Ok(Self { key, inner })
    }

    fn mask(&self, bytes: &[u8]) -> Vec<u8> {
        bytes
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ self.key[index % self.key.len()])
            .collect()
    }
}

impl Codec for XorCodec {
    fn name(&self) -> &str {
        "xor"
    }

    fn open(&mut self, bytes: &[u8]) -> Result<Value> {
        let plain = self.mask(bytes);
        match self.inner {
            XorInner::Json => JsonCodec.open(&plain),
            XorInner::Base64Json => Base64JsonCodec.open(&plain),
        }
    }

    fn seal(&mut self, value: &Value) -> Result<Vec<u8>> {
        let plain = match self.inner {
            XorInner::Json => JsonCodec.seal(value)?,
            XorInner::Base64Json => Base64JsonCodec.seal(value)?,
        };
        Ok(self.mask(&plain))
    }
}

pub struct LiveCodec {
    pub mount: wre_live::Mount,
    pub open_role: String,
    pub seal_role: String,
    name: String,
}

impl LiveCodec {
    pub fn new(mount: wre_live::Mount, open_role: &str, seal_role: &str) -> Result<Self> {
        if !mount.handles.contains_key(open_role) {
            return Err(Error::msg(format!("mount has no {open_role} role")));
        }
        if !mount.handles.contains_key(seal_role) {
            return Err(Error::msg(format!("mount has no {seal_role} role")));
        }

        Ok(Self {
            mount,
            open_role: open_role.to_string(),
            seal_role: seal_role.to_string(),
            name: format!("live:{open_role}/{seal_role}"),
        })
    }
}

impl Codec for LiveCodec {
    fn name(&self) -> &str {
        &self.name
    }

    fn open(&mut self, bytes: &[u8]) -> Result<Value> {
        let text = String::from_utf8_lossy(bytes).into_owned();
        let role = self.open_role.clone();
        self.mount.call(&role, &[Value::String(text)])
    }

    fn seal(&mut self, value: &Value) -> Result<Vec<u8>> {
        let role = self.seal_role.clone();
        let sealed = self.mount.call(&role, &[value.clone()])?;

        match sealed {
            Value::String(text) => Ok(text.into_bytes()),
            Value::Array(items) => Ok(items
                .iter()
                .filter_map(|item| item.as_u64())
                .map(|byte| byte as u8)
                .collect()),
            other => Ok(other.to_string().into_bytes()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundTrip {
    pub codec: String,
    pub opened: bool,
    pub resealed: bool,
    pub identical: bool,
    pub original_sha256: String,
    pub resealed_sha256: Option<String>,
    pub original_len: usize,
    pub resealed_len: Option<usize>,
    #[serde(default)]
    pub note: Option<String>,
}

impl RoundTrip {
    pub fn ok(&self) -> bool {
        self.opened && self.resealed && self.identical
    }
}

pub fn verify_roundtrip(codec: &mut dyn Codec, bytes: &[u8]) -> RoundTrip {
    let original_sha256 = sha256(bytes);
    let name = codec.name().to_string();

    let value = match codec.open(bytes) {
        Ok(value) => value,
        Err(error) => {
            return RoundTrip {
                codec: name,
                opened: false,
                resealed: false,
                identical: false,
                original_sha256,
                resealed_sha256: None,
                original_len: bytes.len(),
                resealed_len: None,
                note: Some(error.to_string()),
            };
        }
    };

    match codec.seal(&value) {
        Ok(resealed) => {
            let resealed_sha256 = sha256(&resealed);
            let identical = resealed_sha256 == original_sha256;
            RoundTrip {
                codec: name,
                opened: true,
                resealed: true,
                identical,
                original_sha256,
                resealed_sha256: Some(resealed_sha256),
                original_len: bytes.len(),
                resealed_len: Some(resealed.len()),
                note: if identical {
                    None
                } else {
                    Some("resealed bytes differ from the original".to_string())
                },
            }
        }
        Err(error) => RoundTrip {
            codec: name,
            opened: true,
            resealed: false,
            identical: false,
            original_sha256,
            resealed_sha256: None,
            original_len: bytes.len(),
            resealed_len: None,
            note: Some(error.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_round_trips() {
        let mut codec = JsonCodec;
        let bytes = br#"{"a":1,"b":[2,3]}"#;
        let report = verify_roundtrip(&mut codec, bytes);
        assert!(report.ok(), "{report:?}");
    }

    #[test]
    fn base64_json_round_trips() {
        let mut codec = Base64JsonCodec;
        let sealed = codec.seal(&json!({ "x": "y" })).unwrap();
        let report = verify_roundtrip(&mut codec, &sealed);
        assert!(report.ok(), "{report:?}");
    }

    #[test]
    fn deflate_json_round_trips() {
        for mut codec in [DeflateJsonCodec::raw(), DeflateJsonCodec::zlib()] {
            let sealed = codec.seal(&json!({ "list": [1, 2, 3], "text": "hello" })).unwrap();
            let report = verify_roundtrip(&mut codec, &sealed);
            assert!(report.ok(), "{} {report:?}", codec.name());
        }
    }

    #[test]
    fn xor_round_trips_and_masks() {
        let mut codec = XorCodec::new(vec![0x5a, 0x21], XorInner::Json).unwrap();
        let sealed = codec.seal(&json!({ "k": 1 })).unwrap();
        assert!(!sealed.starts_with(b"{"));
        let report = verify_roundtrip(&mut codec, &sealed);
        assert!(report.ok(), "{report:?}");
    }

    #[test]
    fn a_failed_open_is_reported_not_panicked() {
        let mut codec = JsonCodec;
        let report = verify_roundtrip(&mut codec, b"not json at all");
        assert!(!report.opened);
        assert!(report.note.is_some());
    }
}
