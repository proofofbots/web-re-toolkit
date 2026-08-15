use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "rule", rename_all = "kebab-case")]
pub enum Accept {
    HexPrefix { prefix: String },
    BytePrefix { prefix: Vec<u8> },
    LeadingZeroBits { bits: u32 },
    ModulusZero { modulus: u32 },
    ScoreAtLeast { nibbles: usize, threshold: f64 },
    Exact { digest: Vec<u8> },
}

impl Default for Accept {
    fn default() -> Self {
        Accept::LeadingZeroBits { bits: 16 }
    }
}

impl Accept {
    pub fn accepts(&self, digest: &[u8]) -> bool {
        match self {
            Accept::HexPrefix { prefix } => hex_prefix(digest, prefix),
            Accept::BytePrefix { prefix } => {
                digest.len() >= prefix.len() && &digest[..prefix.len()] == prefix.as_slice()
            }
            Accept::LeadingZeroBits { bits } => leading_zero_bits(digest) >= *bits,
            Accept::ModulusZero { modulus } => remainder(digest, *modulus) == 0,
            Accept::ScoreAtLeast { nibbles, threshold } => score(digest, *nibbles) >= *threshold,
            Accept::Exact { digest: wanted } => digest == wanted.as_slice(),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Accept::HexPrefix { prefix } => format!("hex starts with {prefix}"),
            Accept::BytePrefix { prefix } => format!("bytes start with {}", hex::encode(prefix)),
            Accept::LeadingZeroBits { bits } => format!("{bits} leading zero bits"),
            Accept::ModulusZero { modulus } => format!("folded remainder mod {modulus} is zero"),
            Accept::ScoreAtLeast { nibbles, threshold } => {
                format!("score over {nibbles} nibbles is at least {threshold}")
            }
            Accept::Exact { digest } => format!("digest equals {}", hex::encode(digest)),
        }
    }
}

pub fn hex_prefix(digest: &[u8], prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }

    if prefix.len() % 2 == 0
        && let Ok(bytes) = hex::decode(prefix)
    {
        return digest.len() >= bytes.len() && digest[..bytes.len()] == bytes[..];
    }

    hex::encode(digest).starts_with(&prefix.to_ascii_lowercase())
}

pub fn leading_zero_bits(digest: &[u8]) -> u32 {
    let mut count = 0;
    for byte in digest {
        count += byte.leading_zeros();
        if *byte != 0 {
            break;
        }
    }
    count
}

pub fn remainder(digest: &[u8], modulus: u32) -> u32 {
    if modulus == 0 {
        return 0;
    }

    let mut value = 0u32;
    for byte in digest {
        value = (value << 8) | u32::from(*byte);
        value %= modulus;
    }
    value
}

pub fn leading_value(digest: &[u8], nibbles: usize) -> u64 {
    let mut value = 0u64;
    for index in 0..nibbles.min(16) {
        let byte = match digest.get(index / 2) {
            Some(byte) => *byte,
            None => break,
        };
        let nibble = if index % 2 == 0 { byte >> 4 } else { byte & 0x0f };
        value = (value << 4) | u64::from(nibble);
    }
    value
}

pub fn score(digest: &[u8], nibbles: usize) -> f64 {
    let value = leading_value(digest, nibbles);
    let ceiling = 2f64.powi((nibbles.min(16) * 4) as i32);
    ceiling / (value as f64 + 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hex_prefix_matches_as_bytes_when_it_is_even() {
        let digest = hex::decode("00ff1234").unwrap();
        assert!(Accept::HexPrefix { prefix: "00ff".to_string() }.accepts(&digest));
        assert!(!Accept::HexPrefix { prefix: "00fe".to_string() }.accepts(&digest));
    }

    #[test]
    fn an_odd_hex_prefix_falls_back_to_string_comparison() {
        let digest = hex::decode("0ab12345").unwrap();
        assert!(Accept::HexPrefix { prefix: "0ab".to_string() }.accepts(&digest));
        assert!(Accept::HexPrefix { prefix: "0AB".to_string() }.accepts(&digest));
        assert!(!Accept::HexPrefix { prefix: "0ac".to_string() }.accepts(&digest));
    }

    #[test]
    fn an_empty_prefix_accepts_anything() {
        assert!(Accept::HexPrefix { prefix: String::new() }.accepts(&[1, 2, 3]));
    }

    #[test]
    fn leading_zero_bits_are_counted_across_bytes() {
        assert_eq!(leading_zero_bits(&[0x00, 0x00, 0x80]), 16);
        assert_eq!(leading_zero_bits(&[0x00, 0x0f]), 12);
        assert_eq!(leading_zero_bits(&[0xff]), 0);
        assert_eq!(leading_zero_bits(&[0x00, 0x00]), 16);
    }

    #[test]
    fn the_folded_remainder_matches_a_big_integer_modulus() {
        let digest = [0x12u8, 0x34, 0x56, 0x78];
        let whole = u32::from_be_bytes(digest);
        for modulus in [3u32, 7, 97, 1009] {
            assert_eq!(remainder(&digest, modulus), whole % modulus);
        }
        assert_eq!(remainder(&digest, 0), 0);
    }

    #[test]
    fn the_leading_value_reads_the_top_nibbles() {
        let digest = hex::decode("0123456789abcdef").unwrap();
        assert_eq!(leading_value(&digest, 1), 0x0);
        assert_eq!(leading_value(&digest, 4), 0x0123);
        assert_eq!(leading_value(&digest, 13), 0x0123456789abc);
    }

    #[test]
    fn a_small_leading_value_scores_high() {
        let easy = hex::decode("0000000000000fffffff").unwrap();
        let hard = hex::decode("ffffffffffffffffffff").unwrap();
        assert!(score(&easy, 13) > score(&hard, 13));

        let rule = Accept::ScoreAtLeast { nibbles: 13, threshold: 1000.0 };
        assert!(rule.accepts(&easy));
        assert!(!rule.accepts(&hard));
    }

    #[test]
    fn every_rule_describes_itself() {
        let rules = [
            Accept::HexPrefix { prefix: "ab".to_string() },
            Accept::BytePrefix { prefix: vec![1] },
            Accept::LeadingZeroBits { bits: 8 },
            Accept::ModulusZero { modulus: 5 },
            Accept::ScoreAtLeast { nibbles: 13, threshold: 2.0 },
            Accept::Exact { digest: vec![9] },
        ];

        for rule in rules {
            assert!(!rule.describe().is_empty());
            let text = serde_json::to_string(&rule).unwrap();
            assert_eq!(serde_json::from_str::<Accept>(&text).unwrap(), rule);
        }
    }
}
