use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use wre_core::error::{Error, Result};

use crate::hpack::Decoder;

pub const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameKind {
    Data,
    Headers,
    Priority,
    ResetStream,
    Settings,
    PushPromise,
    Ping,
    GoAway,
    WindowUpdate,
    Continuation,
    Unknown(u8),
}

impl FrameKind {
    pub fn from_byte(value: u8) -> Self {
        match value {
            0x0 => FrameKind::Data,
            0x1 => FrameKind::Headers,
            0x2 => FrameKind::Priority,
            0x3 => FrameKind::ResetStream,
            0x4 => FrameKind::Settings,
            0x5 => FrameKind::PushPromise,
            0x6 => FrameKind::Ping,
            0x7 => FrameKind::GoAway,
            0x8 => FrameKind::WindowUpdate,
            0x9 => FrameKind::Continuation,
            other => FrameKind::Unknown(other),
        }
    }

    pub fn name(self) -> String {
        match self {
            FrameKind::Data => "DATA".into(),
            FrameKind::Headers => "HEADERS".into(),
            FrameKind::Priority => "PRIORITY".into(),
            FrameKind::ResetStream => "RST_STREAM".into(),
            FrameKind::Settings => "SETTINGS".into(),
            FrameKind::PushPromise => "PUSH_PROMISE".into(),
            FrameKind::Ping => "PING".into(),
            FrameKind::GoAway => "GOAWAY".into(),
            FrameKind::WindowUpdate => "WINDOW_UPDATE".into(),
            FrameKind::Continuation => "CONTINUATION".into(),
            FrameKind::Unknown(value) => format!("UNKNOWN_{value}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub kind: FrameKind,
    pub flags: u8,
    pub stream_id: u32,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn end_headers(&self) -> bool {
        self.flags & 0x4 != 0
    }

    pub fn padded(&self) -> bool {
        self.flags & 0x8 != 0
    }

    pub fn has_priority(&self) -> bool {
        self.flags & 0x20 != 0
    }
}

pub fn parse_frames(bytes: &[u8]) -> Result<Vec<Frame>> {
    let mut cursor = 0usize;

    if bytes.len() >= PREFACE.len() && &bytes[..PREFACE.len()] == PREFACE {
        cursor = PREFACE.len();
    }

    let mut frames = Vec::new();

    while cursor + 9 <= bytes.len() {
        let length = (usize::from(bytes[cursor]) << 16)
            | (usize::from(bytes[cursor + 1]) << 8)
            | usize::from(bytes[cursor + 2]);
        let kind = FrameKind::from_byte(bytes[cursor + 3]);
        let flags = bytes[cursor + 4];
        let stream_id = u32::from_be_bytes([
            bytes[cursor + 5] & 0x7f,
            bytes[cursor + 6],
            bytes[cursor + 7],
            bytes[cursor + 8],
        ]);

        cursor += 9;

        if cursor + length > bytes.len() {
            return Err(Error::msg(format!(
                "h2 frame {} claims {length} bytes, {} remain",
                kind.name(),
                bytes.len() - cursor
            )));
        }

        frames.push(Frame {
            kind,
            flags,
            stream_id,
            payload: bytes[cursor..cursor + length].to_vec(),
        });

        cursor += length;
    }

    Ok(frames)
}

pub const SETTING_NAMES: [(u16, &str); 7] = [
    (0x1, "HEADER_TABLE_SIZE"),
    (0x2, "ENABLE_PUSH"),
    (0x3, "MAX_CONCURRENT_STREAMS"),
    (0x4, "INITIAL_WINDOW_SIZE"),
    (0x5, "MAX_FRAME_SIZE"),
    (0x6, "MAX_HEADER_LIST_SIZE"),
    (0x8, "ENABLE_CONNECT_PROTOCOL"),
];

pub fn setting_name(id: u16) -> String {
    SETTING_NAMES
        .iter()
        .find(|(key, _)| *key == id)
        .map(|(_, name)| (*name).to_string())
        .unwrap_or_else(|| format!("SETTING_{id}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorityRecord {
    pub stream_id: u32,
    pub exclusive: bool,
    pub depends_on: u32,
    pub weight: u16,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct H2Fingerprint {
    pub settings: Vec<(u16, u32)>,
    pub window_update: u32,
    pub priorities: Vec<PriorityRecord>,
    pub pseudo_header_order: Vec<String>,
    pub headers: Vec<(String, String)>,
    pub akamai_text: String,
    pub akamai_hash: String,
}

impl H2Fingerprint {
    pub fn from_frames(frames: &[Frame]) -> Self {
        let mut fingerprint = H2Fingerprint::default();
        let mut decoder = Decoder::new();
        let mut header_block: Vec<u8> = Vec::new();
        let mut collecting = false;

        for frame in frames {
            match frame.kind {
                FrameKind::Settings => {
                    if frame.flags & 0x1 != 0 {
                        continue;
                    }
                    for chunk in frame.payload.chunks_exact(6) {
                        let id = u16::from_be_bytes([chunk[0], chunk[1]]);
                        let value =
                            u32::from_be_bytes([chunk[2], chunk[3], chunk[4], chunk[5]]);
                        fingerprint.settings.push((id, value));
                    }
                }
                FrameKind::WindowUpdate => {
                    if frame.payload.len() >= 4 && frame.stream_id == 0 {
                        fingerprint.window_update = u32::from_be_bytes([
                            frame.payload[0] & 0x7f,
                            frame.payload[1],
                            frame.payload[2],
                            frame.payload[3],
                        ]);
                    }
                }
                FrameKind::Priority => {
                    if frame.payload.len() >= 5 {
                        fingerprint.priorities.push(PriorityRecord {
                            stream_id: frame.stream_id,
                            exclusive: frame.payload[0] & 0x80 != 0,
                            depends_on: u32::from_be_bytes([
                                frame.payload[0] & 0x7f,
                                frame.payload[1],
                                frame.payload[2],
                                frame.payload[3],
                            ]),
                            weight: u16::from(frame.payload[4]) + 1,
                        });
                    }
                }
                FrameKind::Headers => {
                    let mut cursor = 0usize;
                    let mut end = frame.payload.len();

                    if frame.padded() && !frame.payload.is_empty() {
                        let padding = usize::from(frame.payload[0]);
                        cursor += 1;
                        end = end.saturating_sub(padding);
                    }

                    if frame.has_priority() && end >= cursor + 5 {
                        fingerprint.priorities.push(PriorityRecord {
                            stream_id: frame.stream_id,
                            exclusive: frame.payload[cursor] & 0x80 != 0,
                            depends_on: u32::from_be_bytes([
                                frame.payload[cursor] & 0x7f,
                                frame.payload[cursor + 1],
                                frame.payload[cursor + 2],
                                frame.payload[cursor + 3],
                            ]),
                            weight: u16::from(frame.payload[cursor + 4]) + 1,
                        });
                        cursor += 5;
                    }

                    if cursor <= end {
                        header_block.extend_from_slice(&frame.payload[cursor..end]);
                    }

                    collecting = !frame.end_headers();
                    if !collecting {
                        fingerprint.absorb_headers(&mut decoder, &header_block);
                        header_block.clear();
                    }
                }
                FrameKind::Continuation if collecting => {
                    header_block.extend_from_slice(&frame.payload);
                    if frame.end_headers() {
                        collecting = false;
                        fingerprint.absorb_headers(&mut decoder, &header_block);
                        header_block.clear();
                    }
                }
                _ => {}
            }
        }

        fingerprint.finish();
        fingerprint
    }

    fn absorb_headers(&mut self, decoder: &mut Decoder, block: &[u8]) {
        if block.is_empty() || !self.headers.is_empty() {
            return;
        }

        let Ok(headers) = decoder.decode(block) else {
            return;
        };

        for (name, _) in &headers {
            if let Some(pseudo) = name.strip_prefix(':') {
                let short = match pseudo {
                    "method" => "m",
                    "authority" => "a",
                    "scheme" => "s",
                    "path" => "p",
                    other => &other[..1],
                };
                self.pseudo_header_order.push(short.to_string());
            }
        }

        self.headers = headers;
    }

    fn finish(&mut self) {
        let settings = self
            .settings
            .iter()
            .map(|(id, value)| format!("{id}:{value}"))
            .collect::<Vec<_>>()
            .join(";");

        let priorities = if self.priorities.is_empty() {
            "0".to_string()
        } else {
            self.priorities
                .iter()
                .map(|entry| {
                    format!(
                        "{}:{}:{}:{}",
                        entry.stream_id,
                        u8::from(entry.exclusive),
                        entry.depends_on,
                        entry.weight
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        };

        let order = if self.pseudo_header_order.is_empty() {
            "0".to_string()
        } else {
            self.pseudo_header_order.join(",")
        };

        self.akamai_text = format!("{settings}|{}|{priorities}|{order}", self.window_update);

        let mut hasher = Sha256::new();
        hasher.update(self.akamai_text.as_bytes());
        self.akamai_hash = hex::encode(hasher.finalize());
    }

    pub fn describe_settings(&self) -> Vec<(String, u32)> {
        self.settings
            .iter()
            .map(|(id, value)| (setting_name(*id), *value))
            .collect()
    }
}

pub fn fingerprint_bytes(bytes: &[u8]) -> Result<H2Fingerprint> {
    let frames = parse_frames(bytes)?;
    Ok(H2Fingerprint::from_frames(&frames))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hpack::huffman_encode;

    fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(((payload.len() >> 16) & 0xff) as u8);
        out.push(((payload.len() >> 8) & 0xff) as u8);
        out.push((payload.len() & 0xff) as u8);
        out.push(kind);
        out.push(flags);
        out.extend_from_slice(&stream.to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn builds_akamai_fingerprint() {
        let mut settings = Vec::new();
        for (id, value) in [(1u16, 65536u32), (2, 0), (4, 6291456), (6, 262144)] {
            settings.extend_from_slice(&id.to_be_bytes());
            settings.extend_from_slice(&value.to_be_bytes());
        }

        let mut header_block = vec![0x82, 0x87, 0x84];
        let authority = huffman_encode("example.test");
        header_block.push(0x41);
        header_block.push(0x80 | authority.len() as u8);
        header_block.extend_from_slice(&authority);

        let mut stream = Vec::new();
        stream.extend_from_slice(PREFACE);
        stream.extend_from_slice(&frame(0x4, 0, 0, &settings));
        stream.extend_from_slice(&frame(0x8, 0, 0, &15663105u32.to_be_bytes()));
        stream.extend_from_slice(&frame(0x1, 0x4 | 0x1, 1, &header_block));

        let fingerprint = fingerprint_bytes(&stream).unwrap();
        assert_eq!(
            fingerprint.akamai_text,
            "1:65536;2:0;4:6291456;6:262144|15663105|0|m,s,p,a"
        );
        assert_eq!(fingerprint.akamai_hash.len(), 64);
        assert_eq!(fingerprint.headers.len(), 4);
    }
}
