use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Counter {
    #[default]
    Text,
    Uint32Be,
    Uint32Le,
    Uint64Be,
    HexLower,
}

impl Counter {
    pub fn encode(self, nonce: u64) -> Vec<u8> {
        match self {
            Counter::Text => nonce.to_string().into_bytes(),
            Counter::Uint32Be => (nonce as u32).to_be_bytes().to_vec(),
            Counter::Uint32Le => (nonce as u32).to_le_bytes().to_vec(),
            Counter::Uint64Be => nonce.to_be_bytes().to_vec(),
            Counter::HexLower => format!("{nonce:x}").into_bytes(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input {
    #[serde(default)]
    pub prefix: Vec<u8>,
    #[serde(default)]
    pub suffix: Vec<u8>,
    #[serde(default)]
    pub counter: Counter,
}

impl Input {
    pub fn new(prefix: impl Into<Vec<u8>>, counter: Counter) -> Self {
        Self { prefix: prefix.into(), suffix: Vec::new(), counter }
    }

    pub fn suffixed(mut self, suffix: impl Into<Vec<u8>>) -> Self {
        self.suffix = suffix.into();
        self
    }

    pub fn build(&self, nonce: u64) -> Vec<u8> {
        let encoded = self.counter.encode(nonce);
        let mut out = Vec::with_capacity(self.prefix.len() + encoded.len() + self.suffix.len());
        out.extend_from_slice(&self.prefix);
        out.extend_from_slice(&encoded);
        out.extend_from_slice(&self.suffix);
        out
    }

    pub fn joined(parts: &[&[u8]], separator: &[u8], counter: Counter) -> Self {
        let mut prefix = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                prefix.extend_from_slice(separator);
            }
            prefix.extend_from_slice(part);
        }
        Self { prefix, suffix: Vec::new(), counter }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_counter_encoding_is_distinct() {
        assert_eq!(Counter::Text.encode(4_096), b"4096".to_vec());
        assert_eq!(Counter::HexLower.encode(4_096), b"1000".to_vec());
        assert_eq!(Counter::Uint32Be.encode(1), vec![0, 0, 0, 1]);
        assert_eq!(Counter::Uint32Le.encode(1), vec![1, 0, 0, 0]);
        assert_eq!(Counter::Uint64Be.encode(1), vec![0, 0, 0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn a_counter_wraps_at_thirty_two_bits_where_the_encoding_says_so() {
        assert_eq!(Counter::Uint32Be.encode(u32::MAX as u64 + 2), vec![0, 0, 0, 1]);
    }

    #[test]
    fn an_input_wraps_the_nonce_in_its_fixed_parts() {
        let input = Input::new(b"nonce".to_vec(), Counter::Text).suffixed(b"!".to_vec());
        assert_eq!(input.build(7), b"nonce7!".to_vec());
        assert_ne!(input.build(7), input.build(8));
    }

    #[test]
    fn joined_parts_become_one_prefix() {
        let input = Input::joined(&[b"a", b"b", b"c"], b"-", Counter::Text);
        assert_eq!(input.build(1), b"a-b-c1".to_vec());
    }

    #[test]
    fn an_input_round_trips_through_json() {
        let input = Input::new(b"seed".to_vec(), Counter::Uint32Be);
        let text = serde_json::to_string(&input).unwrap();
        assert_eq!(serde_json::from_str::<Input>(&text).unwrap(), input);
    }
}
