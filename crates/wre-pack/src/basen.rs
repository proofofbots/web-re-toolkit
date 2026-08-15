use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

pub const BASE32_LOWER: &str = "0123456789abcdefghijklmnopqrstuv";
pub const BASE32_RFC4648: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
pub const BASE64_STANDARD: &str =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
pub const BASE64_URL: &str =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseN {
    alphabet: Vec<char>,
    bits: u32,
    #[serde(default)]
    pad: Option<char>,
}

impl BaseN {
    pub fn new(alphabet: &str) -> Result<Self> {
        let symbols: Vec<char> = alphabet.chars().collect();

        let bits = match symbols.len() {
            2 => 1,
            4 => 2,
            8 => 3,
            16 => 4,
            32 => 5,
            64 => 6,
            other => {
                return Err(Error::msg(format!(
                    "a bit aligned alphabet has 2, 4, 8, 16, 32 or 64 symbols, this one has {other}"
                )));
            }
        };

        let mut seen = symbols.clone();
        seen.sort_unstable();
        seen.dedup();
        if seen.len() != symbols.len() {
            return Err(Error::msg("the alphabet repeats a symbol"));
        }

        Ok(Self { alphabet: symbols, bits, pad: None })
    }

    pub fn padded(mut self, pad: char) -> Self {
        self.pad = Some(pad);
        self
    }

    pub fn bits(&self) -> u32 {
        self.bits
    }

    pub fn encode(&self, data: &[u8]) -> String {
        let mut out = String::new();
        let mut buffer = 0u32;
        let mut held = 0u32;

        for byte in data {
            buffer = (buffer << 8) | u32::from(*byte);
            held += 8;

            while held >= self.bits {
                held -= self.bits;
                let index = (buffer >> held) & ((1 << self.bits) - 1);
                out.push(self.alphabet[index as usize]);
            }
        }

        if held > 0 {
            let index = (buffer << (self.bits - held)) & ((1 << self.bits) - 1);
            out.push(self.alphabet[index as usize]);
        }

        if let Some(pad) = self.pad {
            let group = lcm(self.bits, 8) / self.bits;
            while out.chars().count() % group as usize != 0 {
                out.push(pad);
            }
        }

        out
    }

    pub fn decode(&self, text: &str) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut buffer = 0u32;
        let mut held = 0u32;

        for symbol in text.chars() {
            if Some(symbol) == self.pad {
                continue;
            }

            let index = self
                .alphabet
                .iter()
                .position(|entry| *entry == symbol)
                .ok_or_else(|| Error::msg(format!("symbol {symbol:?} is not in the alphabet")))?;

            buffer = (buffer << self.bits) | index as u32;
            held += self.bits;

            if held >= 8 {
                held -= 8;
                out.push(((buffer >> held) & 0xff) as u8);
            }
        }

        if held >= self.bits {
            return Err(Error::msg("the text carries a whole unused symbol of padding"));
        }

        if buffer & ((1 << held) - 1) != 0 {
            return Err(Error::msg("the trailing bits of the text are not zero"));
        }

        Ok(out)
    }
}

fn lcm(left: u32, right: u32) -> u32 {
    left / gcd(left, right) * right
}

fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        let next = left % right;
        left = right;
        right = next;
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4648_base32_matches_the_reference_vectors() {
        let codec = BaseN::new(BASE32_RFC4648).unwrap().padded('=');
        assert_eq!(codec.encode(b""), "");
        assert_eq!(codec.encode(b"f"), "MY======");
        assert_eq!(codec.encode(b"fo"), "MZXQ====");
        assert_eq!(codec.encode(b"foo"), "MZXW6===");
        assert_eq!(codec.encode(b"foob"), "MZXW6YQ=");
        assert_eq!(codec.encode(b"fooba"), "MZXW6YTB");
        assert_eq!(codec.encode(b"foobar"), "MZXW6YTBOI======");
    }

    #[test]
    fn base64_matches_the_reference_vectors() {
        let codec = BaseN::new(BASE64_STANDARD).unwrap().padded('=');
        assert_eq!(codec.encode(b"f"), "Zg==");
        assert_eq!(codec.encode(b"fo"), "Zm8=");
        assert_eq!(codec.encode(b"foo"), "Zm9v");
        assert_eq!(codec.encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn every_alphabet_round_trips() {
        let data: Vec<u8> = (0..64u8).collect();

        for alphabet in [BASE32_LOWER, BASE32_RFC4648, BASE64_STANDARD, BASE64_URL] {
            let codec = BaseN::new(alphabet).unwrap();
            let text = codec.encode(&data);
            assert_eq!(codec.decode(&text).unwrap(), data, "alphabet {alphabet}");
        }
    }

    #[test]
    fn padding_is_ignored_on_the_way_back() {
        let codec = BaseN::new(BASE64_STANDARD).unwrap().padded('=');
        assert_eq!(codec.decode("Zm8=").unwrap(), b"fo".to_vec());
        assert_eq!(codec.decode("Zg==").unwrap(), b"f".to_vec());
    }

    #[test]
    fn a_lowercase_base32_alphabet_is_five_bits() {
        let codec = BaseN::new(BASE32_LOWER).unwrap();
        assert_eq!(codec.bits(), 5);
        assert_eq!(codec.encode(&[0x00]), "00");
        assert_eq!(codec.encode(&[0xff]), "vs");
    }

    #[test]
    fn a_misshapen_alphabet_is_rejected() {
        assert!(BaseN::new("abc").is_err());
        assert!(BaseN::new("aabbccdd").is_err());
        assert!(BaseN::new("abcdefgh").is_ok());
    }

    #[test]
    fn an_unknown_symbol_is_reported() {
        let codec = BaseN::new(BASE64_STANDARD).unwrap();
        assert!(codec.decode("Zm9v!").unwrap_err().to_string().contains("not in the alphabet"));
    }

    #[test]
    fn trailing_rubbish_bits_are_reported() {
        let codec = BaseN::new(BASE64_STANDARD).unwrap();
        assert!(codec.decode("Zh").unwrap_err().to_string().contains("not zero"));
    }
}
