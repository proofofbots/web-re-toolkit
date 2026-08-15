use std::io::{Read, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ClientError;
use crate::spec::PROTOCOL_VERSION;

pub const HEADER_LEN: usize = 8;

pub const MAX_JSON_LEN: usize = 64 * 1024 * 1024;
pub const MAX_BIN_LEN: usize = 512 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub json: Vec<u8>,
    pub bin: Vec<u8>,
}

impl Frame {
    pub fn from_envelope(envelope: &Envelope) -> Result<Self, ClientError> {
        let json = serde_json::to_vec(envelope)
            .map_err(|error| ClientError::protocol(format!("envelope encode failed: {error}")))?;
        Ok(Self { json, bin: Vec::new() })
    }

    pub fn with_bin(mut self, bin: Vec<u8>) -> Self {
        self.bin = bin;
        self
    }

    pub fn envelope(&self) -> Result<Envelope, ClientError> {
        serde_json::from_slice(&self.json)
            .map_err(|error| ClientError::protocol(format!("envelope rejected: {error}")))
    }
}

pub fn write_frame(out: &mut impl Write, frame: &Frame) -> std::io::Result<()> {
    let mut header = [0u8; HEADER_LEN];
    header[..4].copy_from_slice(&(frame.json.len() as u32).to_be_bytes());
    header[4..].copy_from_slice(&(frame.bin.len() as u32).to_be_bytes());

    out.write_all(&header)?;
    out.write_all(&frame.json)?;
    if !frame.bin.is_empty() {
        out.write_all(&frame.bin)?;
    }
    out.flush()
}

pub fn read_frame(input: &mut impl Read) -> std::io::Result<Option<Frame>> {
    let mut header = [0u8; HEADER_LEN];

    match read_exact_or_eof(input, &mut header)? {
        false => return Ok(None),
        true => {}
    }

    let json_len = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
    let bin_len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;

    if json_len > MAX_JSON_LEN || bin_len > MAX_BIN_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("frame claims {json_len} json bytes and {bin_len} binary bytes"),
        ));
    }

    let mut json = vec![0u8; json_len];
    input.read_exact(&mut json)?;

    let mut bin = vec![0u8; bin_len];
    if bin_len > 0 {
        input.read_exact(&mut bin)?;
    }

    Ok(Some(Frame { json, bin }))
}

fn read_exact_or_eof(input: &mut impl Read, buffer: &mut [u8]) -> std::io::Result<bool> {
    let mut filled = 0usize;

    while filled < buffer.len() {
        match input.read(&mut buffer[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "frame header cut short",
                ));
            }
            Ok(count) => filled += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }

    Ok(true)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Envelope {
    Req {
        #[serde(default = "protocol_version")]
        v: u32,
        id: u64,
        op: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<String>,
        #[serde(default)]
        params: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deadline_ms: Option<u64>,
    },
    Res {
        #[serde(default = "protocol_version")]
        v: u32,
        id: u64,
        ok: bool,
        #[serde(default, skip_serializing_if = "Value::is_null")]
        result: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<crate::error::ClientError>,
        #[serde(default, skip_serializing_if = "is_zero")]
        took_ms: u64,
    },
    Evt {
        #[serde(default = "protocol_version")]
        v: u32,
        id: u64,
        event: String,
        #[serde(default)]
        data: Value,
    },
    Cancel {
        #[serde(default = "protocol_version")]
        v: u32,
        id: u64,
    },
}

fn protocol_version() -> u32 {
    PROTOCOL_VERSION
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl Envelope {
    pub fn request(id: u64, op: impl Into<String>, params: Value) -> Self {
        Envelope::Req {
            v: PROTOCOL_VERSION,
            id,
            op: op.into(),
            session: None,
            params,
            deadline_ms: None,
        }
    }

    pub fn ok(id: u64, result: Value, took_ms: u64) -> Self {
        Envelope::Res { v: PROTOCOL_VERSION, id, ok: true, result, error: None, took_ms }
    }

    pub fn failed(id: u64, error: ClientError, took_ms: u64) -> Self {
        Envelope::Res {
            v: PROTOCOL_VERSION,
            id,
            ok: false,
            result: Value::Null,
            error: Some(error),
            took_ms,
        }
    }

    pub fn event(id: u64, event: impl Into<String>, data: Value) -> Self {
        Envelope::Evt { v: PROTOCOL_VERSION, id, event: event.into(), data }
    }

    pub fn id(&self) -> u64 {
        match self {
            Envelope::Req { id, .. }
            | Envelope::Res { id, .. }
            | Envelope::Evt { id, .. }
            | Envelope::Cancel { id, .. } => *id,
        }
    }

    pub fn version(&self) -> u32 {
        match self {
            Envelope::Req { v, .. }
            | Envelope::Res { v, .. }
            | Envelope::Evt { v, .. }
            | Envelope::Cancel { v, .. } => *v,
        }
    }
}

pub mod ops {
    pub const HELLO: &str = "hello";
    pub const DESCRIBE: &str = "describe";
    pub const TARGETS: &str = "targets";
    pub const METRICS: &str = "metrics";
    pub const OPEN: &str = "open";
    pub const CLOSE: &str = "close";
    pub const HEALTH: &str = "health";
    pub const WARMUP: &str = "warmup";
    pub const SHUTDOWN: &str = "shutdown";
    pub const DIAG: &str = "diag";

    pub const ALL: [&str; 10] =
        [HELLO, DESCRIBE, TARGETS, METRICS, OPEN, CLOSE, HEALTH, WARMUP, SHUTDOWN, DIAG];

    pub fn is_base(op: &str) -> bool {
        ALL.contains(&op)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenRequest {
    pub target: String,
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub diag: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenReply {
    pub session: String,
    pub target: String,
    pub worker: usize,
    pub ops: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagReply {
    pub target: String,
    pub session: String,
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub report: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReply {
    pub ok: bool,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub detail: Value,
}
