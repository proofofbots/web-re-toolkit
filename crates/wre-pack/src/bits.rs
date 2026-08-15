use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairCharset {
    pub members: Vec<char>,
    #[serde(default = "default_width")]
    pub width: usize,
    #[serde(default = "default_any")]
    pub any: bool,
}

fn default_width() -> usize {
    2
}

fn default_any() -> bool {
    true
}

impl PairCharset {
    pub fn new(members: &str) -> Self {
        Self {
            members: members.chars().collect(),
            width: default_width(),
            any: default_any(),
        }
    }

    pub fn width(mut self, width: usize) -> Self {
        self.width = width.max(1);
        self
    }

    pub fn all_must_match(mut self) -> Self {
        self.any = false;
        self
    }

    pub fn bits(&self, segment: &str) -> Result<Vec<bool>> {
        let symbols: Vec<char> = segment.chars().collect();

        if symbols.len() % self.width != 0 {
            return Err(Error::msg(format!(
                "a segment of {} symbols does not divide into groups of {}",
                symbols.len(),
                self.width
            )));
        }

        Ok(symbols
            .chunks(self.width)
            .map(|group| {
                let hits = group.iter().filter(|symbol| self.members.contains(symbol)).count();
                if self.any { hits > 0 } else { hits == group.len() }
            })
            .collect())
    }

    pub fn flags(&self, segment: &str, names: &[&str]) -> Result<BTreeMap<String, bool>> {
        let bits = self.bits(segment)?;
        Ok(names
            .iter()
            .enumerate()
            .map(|(index, name)| ((*name).to_string(), bits.get(index).copied().unwrap_or(false)))
            .collect())
    }
}

pub fn bits_to_u64(bits: &[bool], most_significant_first: bool) -> u64 {
    let ordered: Vec<bool> = if most_significant_first {
        bits.to_vec()
    } else {
        bits.iter().rev().copied().collect()
    };

    ordered
        .iter()
        .take(64)
        .fold(0u64, |acc, bit| (acc << 1) | u64::from(*bit))
}

pub fn u64_to_bits(value: u64, width: usize, most_significant_first: bool) -> Vec<bool> {
    let mut bits: Vec<bool> = (0..width.min(64))
        .map(|index| (value >> index) & 1 == 1)
        .collect();

    if most_significant_first {
        bits.reverse();
    }

    bits
}

pub fn set_bits(value: u64, names: &[&str]) -> Vec<String> {
    names
        .iter()
        .enumerate()
        .filter(|(index, _)| (value >> index) & 1 == 1)
        .map(|(_, name)| (*name).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VOWELS_AND_ODD_DIGITS: &str = "aeiouy13579";

    #[test]
    fn a_pair_sets_its_bit_when_either_symbol_is_a_member() {
        let charset = PairCharset::new(VOWELS_AND_ODD_DIGITS);
        assert_eq!(charset.bits("ab").unwrap(), vec![true]);
        assert_eq!(charset.bits("ba").unwrap(), vec![true]);
        assert_eq!(charset.bits("bc").unwrap(), vec![false]);
        assert_eq!(charset.bits("abcd3f").unwrap(), vec![true, false, true]);
    }

    #[test]
    fn requiring_every_symbol_narrows_the_result() {
        let charset = PairCharset::new(VOWELS_AND_ODD_DIGITS).all_must_match();
        assert_eq!(charset.bits("ae").unwrap(), vec![true]);
        assert_eq!(charset.bits("ab").unwrap(), vec![false]);
    }

    #[test]
    fn an_odd_length_segment_is_rejected() {
        let charset = PairCharset::new(VOWELS_AND_ODD_DIGITS);
        let error = charset.bits("abc").unwrap_err().to_string();
        assert!(error.contains("does not divide"), "{error}");
    }

    #[test]
    fn a_wider_group_still_works() {
        let charset = PairCharset::new("xy").width(4);
        assert_eq!(charset.bits("abcxdefg").unwrap(), vec![true, false]);
    }

    #[test]
    fn bits_are_named_in_order_and_missing_ones_read_false() {
        let charset = PairCharset::new(VOWELS_AND_ODD_DIGITS);
        let flags = charset.flags("abcd", &["first", "second", "third"]).unwrap();

        assert_eq!(flags.get("first"), Some(&true));
        assert_eq!(flags.get("second"), Some(&false));
        assert_eq!(flags.get("third"), Some(&false));
    }

    #[test]
    fn bits_round_trip_through_an_integer() {
        let bits = vec![true, false, true, true];
        assert_eq!(bits_to_u64(&bits, true), 0b1011);
        assert_eq!(bits_to_u64(&bits, false), 0b1101);
        assert_eq!(u64_to_bits(0b1011, 4, true), bits);
        assert_eq!(u64_to_bits(bits_to_u64(&bits, false), 4, false), bits);
    }

    #[test]
    fn set_bits_are_named_from_the_low_bit_up() {
        let names = ["force-secure", "bot-manager", "proof-of-work", "ip-reputation"];
        assert_eq!(set_bits(0b1010, &names), vec!["bot-manager", "ip-reputation"]);
        assert!(set_bits(0, &names).is_empty());
    }
}
