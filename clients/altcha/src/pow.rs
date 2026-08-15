use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use argon2::{Algorithm, Argon2, Params as Argon2Params, Version};
use hmac::{Hmac, Mac};
use rayon::prelude::*;
use sha2::{Digest, Sha256, Sha384, Sha512};

pub const SHA_256: &str = "SHA-256";
pub const SHA_384: &str = "SHA-384";
pub const SHA_512: &str = "SHA-512";
pub const PBKDF2_SHA_256: &str = "PBKDF2/SHA-256";
pub const PBKDF2_SHA_384: &str = "PBKDF2/SHA-384";
pub const PBKDF2_SHA_512: &str = "PBKDF2/SHA-512";
pub const ARGON2ID: &str = "ARGON2ID";
pub const SCRYPT: &str = "SCRYPT";

pub const ALGORITHMS: [&str; 8] = [
    SHA_256,
    SHA_384,
    SHA_512,
    PBKDF2_SHA_256,
    PBKDF2_SHA_384,
    PBKDF2_SHA_512,
    ARGON2ID,
    SCRYPT,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterMode {
    Uint32,
    Text,
}

#[derive(Debug, Clone)]
pub struct Parameters {
    pub algorithm: String,
    pub nonce: Vec<u8>,
    pub salt: Vec<u8>,
    pub cost: u32,
    pub key_length: usize,
    pub key_prefix: String,
    pub memory_cost: Option<u32>,
    pub parallelism: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Solution {
    pub counter: u64,
    pub derived_key: Vec<u8>,
    pub attempts: u64,
}

pub fn password(nonce: &[u8], counter: u64, mode: CounterMode) -> Vec<u8> {
    let mut out = Vec::with_capacity(nonce.len() + 12);
    out.extend_from_slice(nonce);
    match mode {
        CounterMode::Uint32 => out.extend_from_slice(&(counter as u32).to_be_bytes()),
        CounterMode::Text => out.extend_from_slice(counter.to_string().as_bytes()),
    }
    out
}

pub fn derive_key(parameters: &Parameters, password: &[u8]) -> Result<Vec<u8>, String> {
    let salt = parameters.salt.as_slice();
    let length = parameters.key_length;

    match parameters.algorithm.as_str() {
        SHA_256 => Ok(sha_chain::<Sha256>(salt, password, parameters.cost, length)),
        SHA_384 => Ok(sha_chain::<Sha384>(salt, password, parameters.cost, length)),
        SHA_512 => Ok(sha_chain::<Sha512>(salt, password, parameters.cost, length)),

        PBKDF2_SHA_256 => Ok(pbkdf2_key::<Sha256>(salt, password, parameters.cost, length)),
        PBKDF2_SHA_384 => Ok(pbkdf2_key::<Sha384>(salt, password, parameters.cost, length)),
        PBKDF2_SHA_512 => Ok(pbkdf2_key::<Sha512>(salt, password, parameters.cost, length)),

        SCRYPT => {
            let block_size = parameters.memory_cost.unwrap_or(8);
            let parallelism = parameters.parallelism.unwrap_or(1);
            let log_n = log2_exact(parameters.cost).ok_or_else(|| {
                format!("scrypt cost {} is not a power of two", parameters.cost)
            })?;
            let params = scrypt::Params::new(log_n, block_size, parallelism)
                .map_err(|error| format!("scrypt parameters rejected: {error}"))?;
            let mut out = vec![0u8; length];
            scrypt::scrypt(password, salt, &params, &mut out)
                .map_err(|error| format!("scrypt failed: {error}"))?;
            Ok(out)
        }

        ARGON2ID => {
            let memory_cost = parameters.memory_cost.unwrap_or(16_384);
            let parallelism = parameters.parallelism.unwrap_or(1);
            let params = Argon2Params::new(memory_cost, parameters.cost, parallelism, Some(length))
                .map_err(|error| format!("argon2id parameters rejected: {error}"))?;
            let mut out = vec![0u8; length];
            Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
                .hash_password_into(password, salt, &mut out)
                .map_err(|error| format!("argon2id failed: {error}"))?;
            Ok(out)
        }

        other => Err(format!("unsupported algorithm {other}")),
    }
}

pub fn matches(derived: &[u8], key_prefix: &str) -> bool {
    if key_prefix.len() % 2 == 0 {
        match hex::decode(key_prefix) {
            Ok(prefix) => derived.len() >= prefix.len() && derived[..prefix.len()] == prefix[..],
            Err(_) => hex::encode(derived).starts_with(key_prefix),
        }
    } else {
        hex::encode(derived).starts_with(key_prefix)
    }
}

pub fn solve(
    parameters: &Parameters,
    mode: CounterMode,
    start: u64,
    max_counter: u64,
    workers: usize,
    stop: &(dyn Fn() -> bool + Sync),
) -> Result<Option<Solution>, String> {
    let workers = workers.clamp(1, 64);
    let best = AtomicU64::new(u64::MAX);
    let attempts = AtomicU64::new(0);
    let found: Mutex<Option<Solution>> = Mutex::new(None);
    let failure: Mutex<Option<String>> = Mutex::new(None);

    (0..workers).into_par_iter().for_each(|lane| {
        let mut counter = start + lane as u64;
        let mut local = 0u64;

        while counter <= max_counter {
            if counter >= best.load(Ordering::Relaxed) {
                break;
            }
            if local % 32 == 0 && (stop() || failure.lock().is_ok_and(|slot| slot.is_some())) {
                break;
            }

            let candidate = match derive_key(parameters, &password(&parameters.nonce, counter, mode))
            {
                Ok(candidate) => candidate,
                Err(error) => {
                    if let Ok(mut slot) = failure.lock() {
                        slot.get_or_insert(error);
                    }
                    break;
                }
            };
            local += 1;

            if matches(&candidate, &parameters.key_prefix) {
                best.fetch_min(counter, Ordering::Relaxed);
                if let Ok(mut slot) = found.lock() {
                    let better = slot.as_ref().is_none_or(|current| current.counter > counter);
                    if better {
                        *slot = Some(Solution { counter, derived_key: candidate, attempts: 0 });
                    }
                }
                break;
            }

            counter += workers as u64;
        }

        attempts.fetch_add(local, Ordering::Relaxed);
    });

    if let Some(error) = failure.into_inner().unwrap_or(None) {
        return Err(error);
    }

    let total = attempts.load(Ordering::Relaxed);
    Ok(found
        .into_inner()
        .unwrap_or(None)
        .map(|solution| Solution { attempts: total, ..solution }))
}

fn sha_chain<D: Digest>(salt: &[u8], password: &[u8], cost: u32, length: usize) -> Vec<u8> {
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
