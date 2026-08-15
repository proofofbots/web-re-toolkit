use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};

use wre_core::error::Result;

use crate::kdf::Derivation;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "hash", rename_all = "kebab-case")]
pub enum Hash {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
    HexChain { rounds: usize },
    Derive { derivation: Derivation },
}

impl Default for Hash {
    fn default() -> Self {
        Hash::Sha256
    }
}

impl Hash {
    pub fn digest(&self, input: &[u8]) -> Result<Vec<u8>> {
        match self {
            Hash::Sha1 => Ok(Sha1::digest(input).to_vec()),
            Hash::Sha256 => Ok(Sha256::digest(input).to_vec()),
            Hash::Sha384 => Ok(Sha384::digest(input).to_vec()),
            Hash::Sha512 => Ok(Sha512::digest(input).to_vec()),
            Hash::HexChain { rounds } => Ok(hex::decode(sha256_hex_chain(input, *rounds))
                .unwrap_or_default()),
            Hash::Derive { derivation } => derivation.derive(input),
        }
    }

    pub fn name(&self) -> String {
        match self {
            Hash::Sha1 => "sha1".to_string(),
            Hash::Sha256 => "sha256".to_string(),
            Hash::Sha384 => "sha384".to_string(),
            Hash::Sha512 => "sha512".to_string(),
            Hash::HexChain { rounds } => format!("sha256 hex chain over {rounds} rounds"),
            Hash::Derive { derivation } => derivation.kdf.name().to_string(),
        }
    }
}

pub fn sha256_hex(input: &[u8]) -> String {
    hex::encode(Sha256::digest(input))
}

pub fn sha256_hex_chain(seed: &[u8], rounds: usize) -> String {
    let mut current = sha256_hex(seed);
    for _ in 1..rounds.max(1) {
        current = sha256_hex(current.as_bytes());
    }
    current
}

pub fn sha256_hex_over(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_reference_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha1_matches_the_reference_vector() {
        let digest = Hash::Sha1.digest(b"abc").unwrap();
        assert_eq!(hex::encode(digest), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn a_single_round_chain_is_a_plain_digest() {
        assert_eq!(sha256_hex_chain(b"abc", 1), sha256_hex(b"abc"));
    }

    #[test]
    fn each_extra_round_hashes_the_previous_hex_text() {
        let once = sha256_hex(b"seed");
        let twice = sha256_hex(once.as_bytes());
        assert_eq!(sha256_hex_chain(b"seed", 2), twice);
        assert_eq!(sha256_hex_chain(b"seed", 3), sha256_hex(twice.as_bytes()));
    }

    #[test]
    fn hashing_over_parts_matches_hashing_the_concatenation() {
        assert_eq!(sha256_hex_over(&[b"ab", b"c"]), sha256_hex(b"abc"));
    }

    #[test]
    fn the_chain_variant_returns_the_same_bytes_as_the_hex_form() {
        let digest = Hash::HexChain { rounds: 3 }.digest(b"seed").unwrap();
        assert_eq!(hex::encode(digest), sha256_hex_chain(b"seed", 3));
    }

    #[test]
    fn every_variant_names_itself_and_round_trips() {
        let variants = [
            Hash::Sha1,
            Hash::Sha256,
            Hash::HexChain { rounds: 2 },
            Hash::Derive { derivation: Derivation::default() },
        ];

        for hash in variants {
            assert!(!hash.name().is_empty());
            let text = serde_json::to_string(&hash).unwrap();
            assert_eq!(serde_json::from_str::<Hash>(&text).unwrap(), hash);
        }
    }
}
