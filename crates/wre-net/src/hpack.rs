use wre_core::error::{Error, Result};

pub const STATIC_TABLE: [(&str, &str); 61] = [
    (":authority", ""),
    (":method", "GET"),
    (":method", "POST"),
    (":path", "/"),
    (":path", "/index.html"),
    (":scheme", "http"),
    (":scheme", "https"),
    (":status", "200"),
    (":status", "204"),
    (":status", "206"),
    (":status", "304"),
    (":status", "400"),
    (":status", "404"),
    (":status", "500"),
    ("accept-charset", ""),
    ("accept-encoding", "gzip, deflate"),
    ("accept-language", ""),
    ("accept-ranges", ""),
    ("accept", ""),
    ("access-control-allow-origin", ""),
    ("age", ""),
    ("allow", ""),
    ("authorization", ""),
    ("cache-control", ""),
    ("content-disposition", ""),
    ("content-encoding", ""),
    ("content-language", ""),
    ("content-length", ""),
    ("content-location", ""),
    ("content-range", ""),
    ("content-type", ""),
    ("cookie", ""),
    ("date", ""),
    ("etag", ""),
    ("expect", ""),
    ("expires", ""),
    ("from", ""),
    ("host", ""),
    ("if-match", ""),
    ("if-modified-since", ""),
    ("if-none-match", ""),
    ("if-range", ""),
    ("if-unmodified-since", ""),
    ("last-modified", ""),
    ("link", ""),
    ("location", ""),
    ("max-forwards", ""),
    ("proxy-authenticate", ""),
    ("proxy-authorization", ""),
    ("range", ""),
    ("referer", ""),
    ("refresh", ""),
    ("retry-after", ""),
    ("server", ""),
    ("set-cookie", ""),
    ("strict-transport-security", ""),
    ("transfer-encoding", ""),
    ("user-agent", ""),
    ("vary", ""),
    ("via", ""),
    ("www-authenticate", ""),
];

const HUFFMAN: [(u32, u8); 257] = [
    (0x1ff8, 13), (0x7fffd8, 23), (0xfffffe2, 28), (0xfffffe3, 28), (0xfffffe4, 28),
    (0xfffffe5, 28), (0xfffffe6, 28), (0xfffffe7, 28), (0xfffffe8, 28), (0xffffea, 24),
    (0x3ffffffc, 30), (0xfffffe9, 28), (0xfffffea, 28), (0x3ffffffd, 30), (0xfffffeb, 28),
    (0xfffffec, 28), (0xfffffed, 28), (0xfffffee, 28), (0xfffffef, 28), (0xffffff0, 28),
    (0xffffff1, 28), (0xffffff2, 28), (0x3ffffffe, 30), (0xffffff3, 28), (0xffffff4, 28),
    (0xffffff5, 28), (0xffffff6, 28), (0xffffff7, 28), (0xffffff8, 28), (0xffffff9, 28),
    (0xffffffa, 28), (0xffffffb, 28), (0x14, 6), (0x3f8, 10), (0x3f9, 10),
    (0xffa, 12), (0x1ff9, 13), (0x15, 6), (0xf8, 8), (0x7fa, 11),
    (0x3fa, 10), (0x3fb, 10), (0xf9, 8), (0x7fb, 11), (0xfa, 8),
    (0x16, 6), (0x17, 6), (0x18, 6), (0x0, 5), (0x1, 5),
    (0x2, 5), (0x19, 6), (0x1a, 6), (0x1b, 6), (0x1c, 6),
    (0x1d, 6), (0x1e, 6), (0x1f, 6), (0x5c, 7), (0xfb, 8),
    (0x7ffc, 15), (0x20, 6), (0xffb, 12), (0x3fc, 10), (0x1ffa, 13),
    (0x21, 6), (0x5d, 7), (0x5e, 7), (0x5f, 7), (0x60, 7),
    (0x61, 7), (0x62, 7), (0x63, 7), (0x64, 7), (0x65, 7),
    (0x66, 7), (0x67, 7), (0x68, 7), (0x69, 7), (0x6a, 7),
    (0x6b, 7), (0x6c, 7), (0x6d, 7), (0x6e, 7), (0x6f, 7),
    (0x70, 7), (0x71, 7), (0x72, 7), (0xfc, 8), (0x73, 7),
    (0xfd, 8), (0x1ffb, 13), (0x7fff0, 19), (0x1ffc, 13), (0x3ffc, 14),
    (0x22, 6), (0x7ffd, 15), (0x3, 5), (0x23, 6), (0x4, 5),
    (0x24, 6), (0x5, 5), (0x25, 6), (0x26, 6), (0x27, 6),
    (0x6, 5), (0x74, 7), (0x75, 7), (0x28, 6), (0x29, 6),
    (0x2a, 6), (0x7, 5), (0x2b, 6), (0x76, 7), (0x2c, 6),
    (0x8, 5), (0x9, 5), (0x2d, 6), (0x77, 7), (0x78, 7),
    (0x79, 7), (0x7a, 7), (0x7b, 7), (0x7ffe, 15), (0x7fc, 11),
    (0x3ffd, 14), (0x1ffd, 13), (0xffffffc, 28), (0xfffe6, 20), (0x3fffd2, 22),
    (0xfffe7, 20), (0xfffe8, 20), (0x3fffd3, 22), (0x3fffd4, 22), (0x3fffd5, 22),
    (0x7fffd9, 23), (0x3fffd6, 22), (0x7fffda, 23), (0x7fffdb, 23), (0x7fffdc, 23),
    (0x7fffdd, 23), (0x7fffde, 23), (0xffffeb, 24), (0x7fffdf, 23), (0xffffec, 24),
    (0xffffed, 24), (0x3fffd7, 22), (0x7fffe0, 23), (0xffffee, 24), (0x7fffe1, 23),
    (0x7fffe2, 23), (0x7fffe3, 23), (0x7fffe4, 23), (0x1fffdc, 21), (0x3fffd8, 22),
    (0x7fffe5, 23), (0x3fffd9, 22), (0x7fffe6, 23), (0x7fffe7, 23), (0xffffef, 24),
    (0x3fffda, 22), (0x1fffdd, 21), (0xfffe9, 20), (0x3fffdb, 22), (0x3fffdc, 22),
    (0x7fffe8, 23), (0x7fffe9, 23), (0x1fffde, 21), (0x7fffea, 23), (0x3fffdd, 22),
    (0x3fffde, 22), (0xfffff0, 24), (0x1fffdf, 21), (0x3fffdf, 22), (0x7fffeb, 23),
    (0x7fffec, 23), (0x1fffe0, 21), (0x1fffe1, 21), (0x3fffe0, 22), (0x1fffe2, 21),
    (0x7fffed, 23), (0x3fffe1, 22), (0x7fffee, 23), (0x7fffef, 23), (0xfffea, 20),
    (0x3fffe2, 22), (0x3fffe3, 22), (0x3fffe4, 22), (0x7ffff0, 23), (0x3fffe5, 22),
    (0x3fffe6, 22), (0x7ffff1, 23), (0x3ffffe0, 26), (0x3ffffe1, 26), (0xfffeb, 20),
    (0x7fff1, 19), (0x3fffe7, 22), (0x7ffff2, 23), (0x3fffe8, 22), (0x1ffffec, 25),
    (0x3ffffe2, 26), (0x3ffffe3, 26), (0x3ffffe4, 26), (0x7ffffde, 27), (0x7ffffdf, 27),
    (0x3ffffe5, 26), (0xfffff1, 24), (0x1ffffed, 25), (0x7fff2, 19), (0x1fffe3, 21),
    (0x3ffffe6, 26), (0x7ffffe0, 27), (0x7ffffe1, 27), (0x3ffffe7, 26), (0x7ffffe2, 27),
    (0xfffff2, 24), (0x1fffe4, 21), (0x1fffe5, 21), (0x3ffffe8, 26), (0x3ffffe9, 26),
    (0xffffffd, 28), (0x7ffffe3, 27), (0x7ffffe4, 27), (0x7ffffe5, 27), (0xfffec, 20),
    (0xfffff3, 24), (0xfffed, 20), (0x1fffe6, 21), (0x3fffe9, 22), (0x1fffe7, 21),
    (0x1fffe8, 21), (0x7ffff3, 23), (0x3fffea, 22), (0x3fffeb, 22), (0x1ffffee, 25),
    (0x1ffffef, 25), (0xfffff4, 24), (0xfffff5, 24), (0x3ffffea, 26), (0x7ffff4, 23),
    (0x3ffffeb, 26), (0x7ffffe6, 27), (0x3ffffec, 26), (0x3ffffed, 26), (0x7ffffe7, 27),
    (0x7ffffe8, 27), (0x7ffffe9, 27), (0x7ffffea, 27), (0x7ffffeb, 27), (0xffffffe, 28),
    (0x7ffffec, 27), (0x7ffffed, 27), (0x7ffffee, 27), (0x7ffffef, 27), (0x7fffff0, 27),
    (0x3ffffee, 26), (0x3fffffff, 30),
];

pub fn huffman_decode(bytes: &[u8]) -> Result<String> {
    let mut out = Vec::with_capacity(bytes.len() * 2);
    let mut accumulator: u64 = 0;
    let mut bits: u32 = 0;

    for byte in bytes {
        accumulator = (accumulator << 8) | u64::from(*byte);
        bits += 8;

        loop {
            let Some((symbol, length)) = match_prefix(accumulator, bits) else {
                break;
            };

            if symbol == 256 {
                return Err(Error::msg("hpack huffman end-of-string symbol in literal"));
            }

            out.push(symbol as u8);
            bits -= length;
            accumulator &= (1u64 << bits) - 1;
        }
    }

    if bits > 7 {
        return Err(Error::msg("hpack huffman padding longer than 7 bits"));
    }

    if bits > 0 {
        let padding = accumulator & ((1u64 << bits) - 1);
        if padding != (1u64 << bits) - 1 {
            return Err(Error::msg("hpack huffman padding is not all ones"));
        }
    }

    String::from_utf8(out).map_err(|_| Error::msg("hpack huffman produced invalid utf8"))
}

fn match_prefix(accumulator: u64, bits: u32) -> Option<(usize, u32)> {
    for length in 5..=30u32 {
        if length > bits {
            return None;
        }
        let candidate = (accumulator >> (bits - length)) as u32;
        for (symbol, (code, code_bits)) in HUFFMAN.iter().enumerate() {
            if u32::from(*code_bits) == length && *code == candidate {
                return Some((symbol, length));
            }
        }
    }
    None
}

pub fn huffman_encode(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut accumulator: u64 = 0;
    let mut bits: u32 = 0;

    for byte in text.as_bytes() {
        let (code, length) = HUFFMAN[*byte as usize];
        accumulator = (accumulator << u64::from(length)) | u64::from(code);
        bits += u32::from(length);

        while bits >= 8 {
            out.push(((accumulator >> (bits - 8)) & 0xff) as u8);
            bits -= 8;
        }
    }

    if bits > 0 {
        let padded = (accumulator << (8 - bits)) | ((1u64 << (8 - bits)) - 1);
        out.push((padded & 0xff) as u8);
    }

    out
}

#[derive(Debug, Clone)]
pub struct Decoder {
    dynamic: Vec<(String, String)>,
    capacity: usize,
    size: usize,
}

impl Default for Decoder {
    fn default() -> Self {
        Self { dynamic: Vec::new(), capacity: 4096, size: 0 }
    }
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self { dynamic: Vec::new(), capacity, size: 0 }
    }

    pub fn decode(&mut self, bytes: &[u8]) -> Result<Vec<(String, String)>> {
        let mut out = Vec::new();
        let mut cursor = 0usize;

        while cursor < bytes.len() {
            let first = bytes[cursor];

            if first & 0x80 != 0 {
                let index = read_integer(bytes, &mut cursor, 7)?;
                if index == 0 {
                    return Err(Error::msg("hpack indexed field with index 0"));
                }
                out.push(self.lookup(index)?);
                continue;
            }

            if first & 0xc0 == 0x40 {
                let index = read_integer(bytes, &mut cursor, 6)?;
                let name = self.name_for(index, bytes, &mut cursor)?;
                let value = read_string(bytes, &mut cursor)?;
                self.insert(name.clone(), value.clone());
                out.push((name, value));
                continue;
            }

            if first & 0xe0 == 0x20 {
                let capacity = read_integer(bytes, &mut cursor, 5)?;
                self.resize(capacity);
                continue;
            }

            let index = read_integer(bytes, &mut cursor, 4)?;
            let name = self.name_for(index, bytes, &mut cursor)?;
            let value = read_string(bytes, &mut cursor)?;
            out.push((name, value));
        }

        Ok(out)
    }

    fn name_for(&self, index: usize, bytes: &[u8], cursor: &mut usize) -> Result<String> {
        if index == 0 {
            return read_string(bytes, cursor);
        }
        Ok(self.lookup(index)?.0)
    }

    fn lookup(&self, index: usize) -> Result<(String, String)> {
        if index <= STATIC_TABLE.len() {
            let (name, value) = STATIC_TABLE[index - 1];
            return Ok((name.to_string(), value.to_string()));
        }

        let dynamic_index = index - STATIC_TABLE.len() - 1;
        self.dynamic
            .get(dynamic_index)
            .cloned()
            .ok_or_else(|| Error::msg(format!("hpack index {index} out of range")))
    }

    fn insert(&mut self, name: String, value: String) {
        let entry_size = name.len() + value.len() + 32;
        self.dynamic.insert(0, (name, value));
        self.size += entry_size;
        self.evict();
    }

    fn resize(&mut self, capacity: usize) {
        self.capacity = capacity;
        self.evict();
    }

    fn evict(&mut self) {
        while self.size > self.capacity {
            let Some((name, value)) = self.dynamic.pop() else {
                self.size = 0;
                return;
            };
            self.size = self.size.saturating_sub(name.len() + value.len() + 32);
        }
    }
}

fn read_integer(bytes: &[u8], cursor: &mut usize, prefix_bits: u32) -> Result<usize> {
    if *cursor >= bytes.len() {
        return Err(Error::msg("hpack integer truncated"));
    }

    let mask = (1usize << prefix_bits) - 1;
    let mut value = usize::from(bytes[*cursor]) & mask;
    *cursor += 1;

    if value < mask {
        return Ok(value);
    }

    let mut shift = 0u32;
    loop {
        if *cursor >= bytes.len() {
            return Err(Error::msg("hpack integer continuation truncated"));
        }
        let byte = bytes[*cursor];
        *cursor += 1;
        value += (usize::from(byte & 0x7f)) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            break;
        }
        if shift > 28 {
            return Err(Error::msg("hpack integer too large"));
        }
    }

    Ok(value)
}

fn read_string(bytes: &[u8], cursor: &mut usize) -> Result<String> {
    if *cursor >= bytes.len() {
        return Err(Error::msg("hpack string truncated"));
    }

    let huffman = bytes[*cursor] & 0x80 != 0;
    let length = read_integer(bytes, cursor, 7)?;

    if *cursor + length > bytes.len() {
        return Err(Error::msg("hpack string longer than buffer"));
    }

    let slice = &bytes[*cursor..*cursor + length];
    *cursor += length;

    if huffman {
        huffman_decode(slice)
    } else {
        Ok(String::from_utf8_lossy(slice).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huffman_round_trips() {
        for sample in ["www.example.com", "no-cache", "custom-key", "/index.html", "GET"] {
            let encoded = huffman_encode(sample);
            let decoded = huffman_decode(&encoded).unwrap();
            assert_eq!(decoded, sample);
        }
    }

    #[test]
    fn decodes_rfc_example() {
        let mut decoder = Decoder::new();
        let bytes = hex::decode("828684418cf1e3c2e5f23a6ba0ab90f4ff").unwrap();
        let headers = decoder.decode(&bytes).unwrap();
        assert_eq!(headers[0], (":method".to_string(), "GET".to_string()));
        assert_eq!(headers[1], (":scheme".to_string(), "http".to_string()));
        assert_eq!(headers[2], (":path".to_string(), "/".to_string()));
        assert_eq!(
            headers[3],
            (":authority".to_string(), "www.example.com".to_string())
        );
    }

    #[test]
    fn reads_integers_with_continuation() {
        let bytes = [0x7f, 0x02];
        let mut cursor = 0usize;
        assert_eq!(read_integer(&bytes, &mut cursor, 7).unwrap(), 129);
    }
}
