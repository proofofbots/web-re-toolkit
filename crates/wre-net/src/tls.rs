use md5::{Digest as Md5Digest, Md5};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use wre_core::error::{Error, Result};

pub const GREASE: [u16; 16] = [
    0x0a0a, 0x1a1a, 0x2a2a, 0x3a3a, 0x4a4a, 0x5a5a, 0x6a6a, 0x7a7a, 0x8a8a, 0x9a9a, 0xaaaa, 0xbaba,
    0xcaca, 0xdada, 0xeaea, 0xfafa,
];

pub fn is_grease(value: u16) -> bool {
    GREASE.contains(&value)
}

pub const EXT_SERVER_NAME: u16 = 0x0000;
pub const EXT_SUPPORTED_GROUPS: u16 = 0x000a;
pub const EXT_EC_POINT_FORMATS: u16 = 0x000b;
pub const EXT_SIGNATURE_ALGORITHMS: u16 = 0x000d;
pub const EXT_ALPN: u16 = 0x0010;
pub const EXT_SUPPORTED_VERSIONS: u16 = 0x002b;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientHello {
    pub legacy_version: u16,
    pub random: Vec<u8>,
    pub session_id: Vec<u8>,
    pub cipher_suites: Vec<u16>,
    pub compression_methods: Vec<u8>,
    pub extensions: Vec<Extension>,
    pub server_name: Option<String>,
    pub alpn: Vec<String>,
    pub supported_groups: Vec<u16>,
    pub ec_point_formats: Vec<u8>,
    pub signature_algorithms: Vec<u16>,
    pub supported_versions: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extension {
    pub kind: u16,
    pub data: Vec<u8>,
}

struct Reader<'b> {
    bytes: &'b [u8],
    cursor: usize,
}

impl<'b> Reader<'b> {
    fn new(bytes: &'b [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.cursor)
    }

    fn u8(&mut self) -> Result<u8> {
        if self.remaining() < 1 {
            return Err(Error::msg("client hello truncated"));
        }
        let value = self.bytes[self.cursor];
        self.cursor += 1;
        Ok(value)
    }

    fn u16(&mut self) -> Result<u16> {
        if self.remaining() < 2 {
            return Err(Error::msg("client hello truncated"));
        }
        let value = u16::from_be_bytes([self.bytes[self.cursor], self.bytes[self.cursor + 1]]);
        self.cursor += 2;
        Ok(value)
    }

    fn u24(&mut self) -> Result<u32> {
        if self.remaining() < 3 {
            return Err(Error::msg("client hello truncated"));
        }
        let value = u32::from(self.bytes[self.cursor]) << 16
            | u32::from(self.bytes[self.cursor + 1]) << 8
            | u32::from(self.bytes[self.cursor + 2]);
        self.cursor += 3;
        Ok(value)
    }

    fn take(&mut self, length: usize) -> Result<&'b [u8]> {
        if self.remaining() < length {
            return Err(Error::msg("client hello truncated"));
        }
        let slice = &self.bytes[self.cursor..self.cursor + length];
        self.cursor += length;
        Ok(slice)
    }
}

impl ClientHello {
    pub fn parse_record(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let content_type = reader.u8()?;
        if content_type != 0x16 {
            return Err(Error::msg(format!("not a handshake record: type {content_type:#04x}")));
        }
        let _record_version = reader.u16()?;
        let record_length = reader.u16()? as usize;
        let handshake = reader.take(record_length)?;
        Self::parse_handshake(handshake)
    }

    pub fn parse_handshake(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let message_type = reader.u8()?;
        if message_type != 0x01 {
            return Err(Error::msg(format!(
                "not a client hello: handshake type {message_type:#04x}"
            )));
        }
        let body_length = reader.u24()? as usize;
        let body = reader.take(body_length)?;
        Self::parse_body(body)
    }

    pub fn parse_body(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let mut hello = ClientHello {
            legacy_version: reader.u16()?,
            random: reader.take(32)?.to_vec(),
            ..ClientHello::default()
        };

        let session_length = reader.u8()? as usize;
        hello.session_id = reader.take(session_length)?.to_vec();

        let cipher_length = reader.u16()? as usize;
        let cipher_bytes = reader.take(cipher_length)?;
        hello.cipher_suites = cipher_bytes
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();

        let compression_length = reader.u8()? as usize;
        hello.compression_methods = reader.take(compression_length)?.to_vec();

        if reader.remaining() < 2 {
            return Ok(hello);
        }

        let extensions_length = reader.u16()? as usize;
        let extension_bytes = reader.take(extensions_length)?;
        let mut extension_reader = Reader::new(extension_bytes);

        while extension_reader.remaining() >= 4 {
            let kind = extension_reader.u16()?;
            let length = extension_reader.u16()? as usize;
            let data = extension_reader.take(length)?.to_vec();
            hello.absorb_extension(kind, &data);
            hello.extensions.push(Extension { kind, data });
        }

        Ok(hello)
    }

    fn absorb_extension(&mut self, kind: u16, data: &[u8]) {
        match kind {
            EXT_SERVER_NAME => {
                let mut reader = Reader::new(data);
                if reader.u16().is_err() {
                    return;
                }
                while reader.remaining() >= 3 {
                    let Ok(name_type) = reader.u8() else { return };
                    let Ok(length) = reader.u16() else { return };
                    let Ok(value) = reader.take(length as usize) else {
                        return;
                    };
                    if name_type == 0 {
                        self.server_name = Some(String::from_utf8_lossy(value).into_owned());
                    }
                }
            }
            EXT_ALPN => {
                let mut reader = Reader::new(data);
                if reader.u16().is_err() {
                    return;
                }
                while reader.remaining() >= 1 {
                    let Ok(length) = reader.u8() else { return };
                    let Ok(value) = reader.take(length as usize) else {
                        return;
                    };
                    self.alpn.push(String::from_utf8_lossy(value).into_owned());
                }
            }
            EXT_SUPPORTED_GROUPS => {
                let mut reader = Reader::new(data);
                if reader.u16().is_err() {
                    return;
                }
                while reader.remaining() >= 2 {
                    if let Ok(group) = reader.u16() {
                        self.supported_groups.push(group);
                    }
                }
            }
            EXT_EC_POINT_FORMATS => {
                let mut reader = Reader::new(data);
                let Ok(length) = reader.u8() else { return };
                if let Ok(values) = reader.take(length as usize) {
                    self.ec_point_formats = values.to_vec();
                }
            }
            EXT_SIGNATURE_ALGORITHMS => {
                let mut reader = Reader::new(data);
                if reader.u16().is_err() {
                    return;
                }
                while reader.remaining() >= 2 {
                    if let Ok(algorithm) = reader.u16() {
                        self.signature_algorithms.push(algorithm);
                    }
                }
            }
            EXT_SUPPORTED_VERSIONS => {
                let mut reader = Reader::new(data);
                let Ok(length) = reader.u8() else { return };
                if let Ok(values) = reader.take(length as usize) {
                    for pair in values.chunks_exact(2) {
                        self.supported_versions
                            .push(u16::from_be_bytes([pair[0], pair[1]]));
                    }
                }
            }
            _ => {}
        }
    }

    pub fn extension_kinds(&self) -> Vec<u16> {
        self.extensions.iter().map(|extension| extension.kind).collect()
    }

    pub fn negotiated_version(&self) -> u16 {
        self.supported_versions
            .iter()
            .copied()
            .filter(|version| !is_grease(*version))
            .max()
            .unwrap_or(self.legacy_version)
    }

    pub fn ja3(&self) -> Ja3 {
        let ciphers: Vec<u16> = self
            .cipher_suites
            .iter()
            .copied()
            .filter(|value| !is_grease(*value))
            .collect();
        let extensions: Vec<u16> = self
            .extension_kinds()
            .into_iter()
            .filter(|value| !is_grease(*value))
            .collect();
        let groups: Vec<u16> = self
            .supported_groups
            .iter()
            .copied()
            .filter(|value| !is_grease(*value))
            .collect();

        let text = format!(
            "{},{},{},{},{}",
            self.legacy_version,
            join(&ciphers),
            join(&extensions),
            join(&groups),
            self.ec_point_formats
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join("-")
        );

        let mut hasher = Md5::new();
        hasher.update(text.as_bytes());
        let hash = hex::encode(hasher.finalize());

        Ja3 { text, hash }
    }

    pub fn ja4(&self) -> String {
        let version = match self.negotiated_version() {
            0x0304 => "13",
            0x0303 => "12",
            0x0302 => "11",
            0x0301 => "10",
            0x0300 => "s3",
            _ => "00",
        };

        let sni = if self.server_name.is_some() { "d" } else { "i" };

        let ciphers: Vec<u16> = self
            .cipher_suites
            .iter()
            .copied()
            .filter(|value| !is_grease(*value))
            .collect();

        let extension_kinds: Vec<u16> = self
            .extension_kinds()
            .into_iter()
            .filter(|value| !is_grease(*value))
            .collect();

        let alpn = self
            .alpn
            .iter()
            .find(|value| !value.is_empty())
            .map(|value| {
                let chars: Vec<char> = value.chars().collect();
                let first = chars[0];
                let last = chars[chars.len() - 1];
                if first.is_ascii() && last.is_ascii() {
                    format!("{first}{last}")
                } else {
                    let bytes = value.as_bytes();
                    format!(
                        "{:x}{:x}",
                        bytes[0] >> 4,
                        bytes[bytes.len() - 1] & 0x0f
                    )
                }
            })
            .unwrap_or_else(|| "00".to_string());

        let prefix = format!(
            "t{version}{sni}{:02}{:02}{alpn}",
            ciphers.len().min(99),
            extension_kinds.len().min(99)
        );

        let mut sorted_ciphers = ciphers.clone();
        sorted_ciphers.sort_unstable();
        let cipher_part = truncated_sha256(&hex_join(&sorted_ciphers), ciphers.is_empty());

        let mut sorted_extensions: Vec<u16> = extension_kinds
            .iter()
            .copied()
            .filter(|kind| *kind != EXT_SERVER_NAME && *kind != EXT_ALPN)
            .collect();
        sorted_extensions.sort_unstable();

        let signatures: Vec<u16> = self
            .signature_algorithms
            .iter()
            .copied()
            .filter(|value| !is_grease(*value))
            .collect();

        let extension_text = if signatures.is_empty() {
            hex_join(&sorted_extensions)
        } else {
            format!("{}_{}", hex_join(&sorted_extensions), hex_join(&signatures))
        };

        let extension_part = truncated_sha256(&extension_text, extension_kinds.is_empty());

        format!("{prefix}_{cipher_part}_{extension_part}")
    }

    pub fn summary(&self) -> ClientHelloSummary {
        ClientHelloSummary {
            ja3: self.ja3(),
            ja4: self.ja4(),
            version: self.negotiated_version(),
            server_name: self.server_name.clone(),
            alpn: self.alpn.clone(),
            cipher_count: self.cipher_suites.len(),
            extension_count: self.extensions.len(),
            grease_present: self.cipher_suites.iter().any(|value| is_grease(*value))
                || self.extension_kinds().iter().any(|value| is_grease(*value)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ja3 {
    pub text: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientHelloSummary {
    pub ja3: Ja3,
    pub ja4: String,
    pub version: u16,
    pub server_name: Option<String>,
    pub alpn: Vec<String>,
    pub cipher_count: usize,
    pub extension_count: usize,
    pub grease_present: bool,
}

fn join(values: &[u16]) -> String {
    values
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("-")
}

fn hex_join(values: &[u16]) -> String {
    values
        .iter()
        .map(|value| format!("{value:04x}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn truncated_sha256(text: &str, empty: bool) -> String {
    if empty {
        return "000000000000".to_string();
    }
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())[..12].to_string()
}

#[derive(Debug, Clone)]
pub struct ClientHelloBuilder {
    pub legacy_version: u16,
    pub random: [u8; 32],
    pub session_id: Vec<u8>,
    pub cipher_suites: Vec<u16>,
    pub compression_methods: Vec<u8>,
    pub extensions: Vec<Extension>,
}

impl Default for ClientHelloBuilder {
    fn default() -> Self {
        Self {
            legacy_version: 0x0303,
            random: [0u8; 32],
            session_id: vec![0u8; 32],
            cipher_suites: Vec::new(),
            compression_methods: vec![0],
            extensions: Vec::new(),
        }
    }
}

impl ClientHelloBuilder {
    pub fn cipher(mut self, suite: u16) -> Self {
        self.cipher_suites.push(suite);
        self
    }

    pub fn extension(mut self, kind: u16, data: Vec<u8>) -> Self {
        self.extensions.push(Extension { kind, data });
        self
    }

    pub fn server_name(self, host: &str) -> Self {
        let name = host.as_bytes();
        let mut data = Vec::with_capacity(name.len() + 5);
        data.extend_from_slice(&((name.len() + 3) as u16).to_be_bytes());
        data.push(0);
        data.extend_from_slice(&(name.len() as u16).to_be_bytes());
        data.extend_from_slice(name);
        self.extension(EXT_SERVER_NAME, data)
    }

    pub fn alpn(self, protocols: &[&str]) -> Self {
        let mut list = Vec::new();
        for protocol in protocols {
            list.push(protocol.len() as u8);
            list.extend_from_slice(protocol.as_bytes());
        }
        let mut data = Vec::with_capacity(list.len() + 2);
        data.extend_from_slice(&(list.len() as u16).to_be_bytes());
        data.extend_from_slice(&list);
        self.extension(EXT_ALPN, data)
    }

    pub fn supported_groups(self, groups: &[u16]) -> Self {
        let mut list = Vec::new();
        for group in groups {
            list.extend_from_slice(&group.to_be_bytes());
        }
        let mut data = Vec::with_capacity(list.len() + 2);
        data.extend_from_slice(&(list.len() as u16).to_be_bytes());
        data.extend_from_slice(&list);
        self.extension(EXT_SUPPORTED_GROUPS, data)
    }

    pub fn supported_versions(self, versions: &[u16]) -> Self {
        let mut list = Vec::new();
        for version in versions {
            list.extend_from_slice(&version.to_be_bytes());
        }
        let mut data = Vec::with_capacity(list.len() + 1);
        data.push(list.len() as u8);
        data.extend_from_slice(&list);
        self.extension(EXT_SUPPORTED_VERSIONS, data)
    }

    pub fn signature_algorithms(self, algorithms: &[u16]) -> Self {
        let mut list = Vec::new();
        for algorithm in algorithms {
            list.extend_from_slice(&algorithm.to_be_bytes());
        }
        let mut data = Vec::with_capacity(list.len() + 2);
        data.extend_from_slice(&(list.len() as u16).to_be_bytes());
        data.extend_from_slice(&list);
        self.extension(EXT_SIGNATURE_ALGORITHMS, data)
    }

    pub fn body(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.legacy_version.to_be_bytes());
        out.extend_from_slice(&self.random);
        out.push(self.session_id.len() as u8);
        out.extend_from_slice(&self.session_id);

        let mut ciphers = Vec::new();
        for suite in &self.cipher_suites {
            ciphers.extend_from_slice(&suite.to_be_bytes());
        }
        out.extend_from_slice(&(ciphers.len() as u16).to_be_bytes());
        out.extend_from_slice(&ciphers);

        out.push(self.compression_methods.len() as u8);
        out.extend_from_slice(&self.compression_methods);

        let mut extensions = Vec::new();
        for extension in &self.extensions {
            extensions.extend_from_slice(&extension.kind.to_be_bytes());
            extensions.extend_from_slice(&(extension.data.len() as u16).to_be_bytes());
            extensions.extend_from_slice(&extension.data);
        }
        out.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
        out.extend_from_slice(&extensions);

        out
    }

    pub fn handshake(&self) -> Vec<u8> {
        let body = self.body();
        let mut out = Vec::with_capacity(body.len() + 4);
        out.push(0x01);
        out.push(((body.len() >> 16) & 0xff) as u8);
        out.push(((body.len() >> 8) & 0xff) as u8);
        out.push((body.len() & 0xff) as u8);
        out.extend_from_slice(&body);
        out
    }

    pub fn record(&self) -> Vec<u8> {
        let handshake = self.handshake();
        let mut out = Vec::with_capacity(handshake.len() + 5);
        out.push(0x16);
        out.extend_from_slice(&0x0301u16.to_be_bytes());
        out.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
        out.extend_from_slice(&handshake);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_built_hello() {
        let built = ClientHelloBuilder::default()
            .cipher(0x1301)
            .cipher(0x1302)
            .cipher(0xc02b)
            .server_name("example.test")
            .alpn(&["h2", "http/1.1"])
            .supported_groups(&[0x001d, 0x0017])
            .supported_versions(&[0x0304, 0x0303])
            .signature_algorithms(&[0x0403, 0x0804]);

        let parsed = ClientHello::parse_record(&built.record()).unwrap();
        assert_eq!(parsed.cipher_suites, vec![0x1301, 0x1302, 0xc02b]);
        assert_eq!(parsed.server_name.as_deref(), Some("example.test"));
        assert_eq!(parsed.alpn, vec!["h2".to_string(), "http/1.1".to_string()]);
        assert_eq!(parsed.supported_groups, vec![0x001d, 0x0017]);
        assert_eq!(parsed.negotiated_version(), 0x0304);

        let ja4 = parsed.ja4();
        assert!(ja4.starts_with("t13d0305h2_"), "unexpected ja4 {ja4}");
        assert_eq!(ja4.split('_').count(), 3);

        let ja3 = parsed.ja3();
        assert_eq!(ja3.hash.len(), 32);
        assert!(ja3.text.starts_with("771,4865-4866-49195,"));
    }

    #[test]
    fn drops_grease_from_ja3() {
        let built = ClientHelloBuilder::default()
            .cipher(0x0a0a)
            .cipher(0x1301)
            .supported_groups(&[0x1a1a, 0x001d]);

        let parsed = ClientHello::parse_record(&built.record()).unwrap();
        let ja3 = parsed.ja3();
        assert!(!ja3.text.contains("2570"));
        assert!(ja3.text.contains("4865"));
    }
}
