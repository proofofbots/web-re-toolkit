use argon2::{Algorithm, Argon2, Params as Argon2Params, Version};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha384, Sha512};

use wre_core::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kdf {
    #[default]
    Sha256,
    Sha384,
    Sha512,
    Pbkdf2Sha256,
    Pbkdf2Sha384,
    Pbkdf2Sha512,
    Scrypt,
    Argon2id,
}

impl Kdf {
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_uppercase().replace('_', "-").as_str() {
            "SHA-256" | "SHA256" => Some(Kdf::Sha256),
            "SHA-384" | "SHA384" => Some(Kdf::Sha384),
            "SHA-512" | "SHA512" => Some(Kdf::Sha512),
            "PBKDF2/SHA-256" | "PBKDF2-SHA-256" => Some(Kdf::Pbkdf2Sha256),
            "PBKDF2/SHA-384" | "PBKDF2-SHA-384" => Some(Kdf::Pbkdf2Sha384),
            "PBKDF2/SHA-512" | "PBKDF2-SHA-512" => Some(Kdf::Pbkdf2Sha512),
            "SCRYPT" => Some(Kdf::Scrypt),
            "ARGON2ID" => Some(Kdf::Argon2id),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kdf::Sha256 => "SHA-256",
            Kdf::Sha384 => "SHA-384",
            Kdf::Sha512 => "SHA-512",
            Kdf::Pbkdf2Sha256 => "PBKDF2/SHA-256",
            Kdf::Pbkdf2Sha384 => "PBKDF2/SHA-384",
            Kdf::Pbkdf2Sha512 => "PBKDF2/SHA-512",
            Kdf::Scrypt => "SCRYPT",
            Kdf::Argon2id => "ARGON2ID",
        }
    }

    pub fn all() -> [Kdf; 8] {
        [
            Kdf::Sha256,
            Kdf::Sha384,
            Kdf::Sha512,
            Kdf::Pbkdf2Sha256,
            Kdf::Pbkdf2Sha384,
            Kdf::Pbkdf2Sha512,
            Kdf::Scrypt,
            Kdf::Argon2id,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Derivation {
    pub kdf: Kdf,
    #[serde(default)]
    pub salt: Vec<u8>,
    #[serde(default = "one")]
    pub cost: u32,
    #[serde(default = "thirty_two")]
    pub key_length: usize,
    #[serde(default)]
    pub memory_cost: Option<u32>,
    #[serde(default)]
    pub parallelism: Option<u32>,
}

fn one() -> u32 {
    1
}

fn thirty_two() -> usize {
    32
}

impl Default for Derivation {
    fn default() -> Self {
        Self {
            kdf: Kdf::Sha256,
            salt: Vec::new(),
            cost: 1,
            key_length: 32,
            memory_cost: None,
            parallelism: None,
        }
    }
}

impl Derivation {
    pub fn new(kdf: Kdf, salt: Vec<u8>, cost: u32, key_length: usize) -> Self {
        Self { kdf, salt, cost, key_length, ..Self::default() }
    }

    pub fn derive(&self, password: &[u8]) -> Result<Vec<u8>> {
        let salt = self.salt.as_slice();
        let length = self.key_length;

        match self.kdf {
            Kdf::Sha256 => Ok(chain::<Sha256>(salt, password, self.cost, length)),
            Kdf::Sha384 => Ok(chain::<Sha384>(salt, password, self.cost, length)),
            Kdf::Sha512 => Ok(chain::<Sha512>(salt, password, self.cost, length)),

            Kdf::Pbkdf2Sha256 => Ok(pbkdf2_key::<Sha256>(salt, password, self.cost, length)),
            Kdf::Pbkdf2Sha384 => Ok(pbkdf2_key::<Sha384>(salt, password, self.cost, length)),
            Kdf::Pbkdf2Sha512 => Ok(pbkdf2_key::<Sha512>(salt, password, self.cost, length)),

            Kdf::Scrypt => {
                let block_size = self.memory_cost.unwrap_or(8);
                let parallelism = self.parallelism.unwrap_or(1);
                let log_n = log2_exact(self.cost).ok_or_else(|| {
                    Error::msg(format!("scrypt cost {} is not a power of two", self.cost))
                })?;

                let params = scrypt::Params::new(log_n, block_size, parallelism)
                    .map_err(|error| Error::msg(format!("scrypt parameters rejected: {error}")))?;

                let mut out = vec![0u8; length];
                scrypt::scrypt(password, salt, &params, &mut out)
                    .map_err(|error| Error::msg(format!("scrypt failed: {error}")))?;
                Ok(out)
            }

            Kdf::Argon2id => {
                let memory_cost = self.memory_cost.unwrap_or(16_384);
                let parallelism = self.parallelism.unwrap_or(1);

                let params = Argon2Params::new(memory_cost, self.cost, parallelism, Some(length))
                    .map_err(|error| {
                        Error::msg(format!("argon2id parameters rejected: {error}"))
                    })?;

                let mut out = vec![0u8; length];
                Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
                    .hash_password_into(password, salt, &mut out)
                    .map_err(|error| Error::msg(format!("argon2id failed: {error}")))?;
                Ok(out)
            }
        }
    }
}

fn chain<D: Digest>(salt: &[u8], password: &[u8], cost: u32, length: usize) -> Vec<u8> {
    let iterations = cost.max(1);
    let mut data = Vec::with_capacity(salt.len() + password.len());
    data.extend_from_slice(salt);
    data.extend_from_slice(password);

    let mut derived = Vec::new();
    for _ in 0..iterations {
        let digest = D::digest(&data);
        derived = digest[..length.min(digest.len())].to_vec();
        data = derived.clone();
    }

    derived
}

fn pbkdf2_key<D>(salt: &[u8], password: &[u8], cost: u32, length: usize) -> Vec<u8>
where
    D: Digest + hmac::EagerHash,
    Hmac<D>: Mac,
{
    let mut out = vec![0u8; length];
    pbkdf2::pbkdf2_hmac::<D>(password, salt, cost.max(1), &mut out);
    out
}

fn log2_exact(value: u32) -> Option<u8> {
    if value == 0 || !value.is_power_of_two() {
        return None;
    }
    Some(value.trailing_zeros() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_round_trips() {
        for kdf in Kdf::all() {
            assert_eq!(Kdf::parse(kdf.name()), Some(kdf));
        }
        assert_eq!(Kdf::parse("sha-256"), Some(Kdf::Sha256));
        assert_eq!(Kdf::parse("nonsense"), None);
    }

    #[test]
    fn a_derivation_is_deterministic_and_the_asked_for_length() {
        let derivation = Derivation::new(Kdf::Sha256, b"salt".to_vec(), 10, 32);
        let first = derivation.derive(b"password").unwrap();
        assert_eq!(first.len(), 32);
        assert_eq!(derivation.derive(b"password").unwrap(), first);
        assert_ne!(derivation.derive(b"other").unwrap(), first);
    }

    #[test]
    fn pbkdf2_matches_the_reference_vector() {
        let derivation = Derivation::new(Kdf::Pbkdf2Sha256, b"salt".to_vec(), 1, 32);
        assert_eq!(
            hex::encode(derivation.derive(b"password").unwrap()),
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b"
        );
    }

    #[test]
    fn the_cost_changes_the_output() {
        let cheap = Derivation::new(Kdf::Sha256, b"salt".to_vec(), 1, 32);
        let dear = Derivation::new(Kdf::Sha256, b"salt".to_vec(), 2, 32);
        assert_ne!(cheap.derive(b"x").unwrap(), dear.derive(b"x").unwrap());
    }

    #[test]
    fn scrypt_rejects_a_cost_that_is_not_a_power_of_two() {
        let derivation = Derivation::new(Kdf::Scrypt, b"salt".to_vec(), 1000, 32);
        assert!(derivation.derive(b"password").is_err());
    }

    #[test]
    fn the_memory_hard_functions_still_produce_a_key() {
        let mut derivation = Derivation::new(Kdf::Scrypt, b"salt".to_vec(), 16, 32);
        derivation.memory_cost = Some(8);
        assert_eq!(derivation.derive(b"password").unwrap().len(), 32);

        let mut derivation = Derivation::new(Kdf::Argon2id, b"a salt long enough".to_vec(), 1, 32);
        derivation.memory_cost = Some(64);
        assert_eq!(derivation.derive(b"password").unwrap().len(), 32);
    }
}
