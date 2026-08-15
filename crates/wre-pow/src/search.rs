use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

use crate::hash::Hash;
use crate::input::Input;
use crate::predicate::Accept;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Challenge {
    pub input: Input,
    #[serde(default)]
    pub hash: Hash,
    pub accept: Accept,
    #[serde(default)]
    pub start: u64,
    #[serde(default = "default_ceiling")]
    pub ceiling: u64,
    #[serde(default = "default_workers")]
    pub workers: usize,
}

fn default_ceiling() -> u64 {
    5_000_000
}

fn default_workers() -> usize {
    4
}

impl Challenge {
    pub fn new(input: Input, accept: Accept) -> Self {
        Self {
            input,
            hash: Hash::default(),
            accept,
            start: 0,
            ceiling: default_ceiling(),
            workers: default_workers(),
        }
    }

    pub fn hashed(mut self, hash: Hash) -> Self {
        self.hash = hash;
        self
    }

    pub fn workers(mut self, workers: usize) -> Self {
        self.workers = workers;
        self
    }

    pub fn ceiling(mut self, ceiling: u64) -> Self {
        self.ceiling = ceiling;
        self
    }

    pub fn verify(&self, nonce: u64) -> Result<bool> {
        let digest = self.hash.digest(&self.input.build(nonce))?;
        Ok(self.accept.accepts(&digest))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Solution {
    pub nonce: u64,
    pub digest: Vec<u8>,
    pub attempts: u64,
    pub elapsed_ms: u64,
}

impl Solution {
    pub fn digest_hex(&self) -> String {
        hex::encode(&self.digest)
    }
}

pub fn solve(challenge: &Challenge, stop: &(dyn Fn() -> bool + Sync)) -> Result<Option<Solution>> {
    let started = Instant::now();
    let workers = challenge.workers.clamp(1, 256);
    let best = AtomicU64::new(u64::MAX);
    let attempts = AtomicU64::new(0);
    let found: Mutex<Option<Solution>> = Mutex::new(None);
    let failure: Mutex<Option<String>> = Mutex::new(None);

    (0..workers).into_par_iter().for_each(|lane| {
        let mut nonce = challenge.start.saturating_add(lane as u64);
        let mut local = 0u64;

        while nonce <= challenge.ceiling {
            if nonce >= best.load(Ordering::Relaxed) {
                break;
            }
            if local % 64 == 0 && (stop() || failure.lock().is_ok_and(|slot| slot.is_some())) {
                break;
            }

            let digest = match challenge.hash.digest(&challenge.input.build(nonce)) {
                Ok(digest) => digest,
                Err(error) => {
                    if let Ok(mut slot) = failure.lock() {
                        slot.get_or_insert(error.to_string());
                    }
                    break;
                }
            };
            local += 1;

            if challenge.accept.accepts(&digest) {
                best.fetch_min(nonce, Ordering::Relaxed);
                if let Ok(mut slot) = found.lock() {
                    let better = slot.as_ref().is_none_or(|current| current.nonce > nonce);
                    if better {
                        *slot = Some(Solution {
                            nonce,
                            digest,
                            attempts: 0,
                            elapsed_ms: 0,
                        });
                    }
                }
                break;
            }

            nonce = nonce.saturating_add(workers as u64);
        }

        attempts.fetch_add(local, Ordering::Relaxed);
    });

    if let Some(error) = failure.into_inner().unwrap_or(None) {
        return Err(Error::msg(error));
    }

    let total = attempts.load(Ordering::Relaxed);
    let elapsed_ms = started.elapsed().as_millis() as u64;

    Ok(found
        .into_inner()
        .unwrap_or(None)
        .map(|solution| Solution { attempts: total, elapsed_ms, ..solution }))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rounds {
    pub rounds: Vec<Challenge>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundsSolution {
    pub solutions: Vec<Solution>,
}

impl RoundsSolution {
    pub fn nonces(&self) -> Vec<u64> {
        self.solutions.iter().map(|solution| solution.nonce).collect()
    }

    pub fn attempts(&self) -> Vec<u64> {
        self.solutions.iter().map(|solution| solution.attempts).collect()
    }

    pub fn elapsed_ms(&self) -> Vec<u64> {
        self.solutions.iter().map(|solution| solution.elapsed_ms).collect()
    }

    pub fn total_attempts(&self) -> u64 {
        self.solutions.iter().map(|solution| solution.attempts).sum()
    }
}

impl Rounds {
    pub fn stepping_modulus(base: Challenge, count: usize, first_modulus: u32) -> Self {
        let rounds = (0..count)
            .map(|index| {
                let mut round = base.clone();
                round.accept = Accept::ModulusZero {
                    modulus: first_modulus.saturating_add(index as u32),
                };
                round
            })
            .collect();

        Self { rounds }
    }

    pub fn solve(&self, stop: &(dyn Fn() -> bool + Sync)) -> Result<Option<RoundsSolution>> {
        let mut solutions = Vec::with_capacity(self.rounds.len());

        for (index, round) in self.rounds.iter().enumerate() {
            match solve(round, stop)? {
                Some(solution) => solutions.push(solution),
                None => {
                    return Err(Error::msg(format!(
                        "round {index} found no nonce below {}",
                        round.ceiling
                    )));
                }
            }
        }

        Ok(Some(RoundsSolution { solutions }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Counter;

    fn never() -> impl Fn() -> bool + Sync {
        || false
    }

    fn challenge(bits: u32) -> Challenge {
        Challenge::new(
            Input::new(b"session-seed-".to_vec(), Counter::Text),
            Accept::LeadingZeroBits { bits },
        )
        .workers(4)
        .ceiling(2_000_000)
    }

    #[test]
    fn a_solved_nonce_verifies() {
        let challenge = challenge(12);
        let solution = solve(&challenge, &never()).unwrap().expect("no nonce found");

        assert!(challenge.verify(solution.nonce).unwrap());
        assert!(solution.attempts > 0);
        assert_eq!(solution.digest.len(), 32);
    }

    #[test]
    fn the_lowest_accepting_nonce_is_returned() {
        let challenge = challenge(10);
        let solution = solve(&challenge, &never()).unwrap().unwrap();

        for nonce in challenge.start..solution.nonce {
            assert!(!challenge.verify(nonce).unwrap(), "nonce {nonce} was skipped");
        }
    }

    #[test]
    fn worker_count_does_not_change_the_answer() {
        let one = solve(&challenge(12).workers(1), &never()).unwrap().unwrap();
        let many = solve(&challenge(12).workers(8), &never()).unwrap().unwrap();
        assert_eq!(one.nonce, many.nonce);
    }

    #[test]
    fn a_ceiling_that_is_too_low_finds_nothing() {
        let challenge = challenge(32).ceiling(200);
        assert!(solve(&challenge, &never()).unwrap().is_none());
    }

    #[test]
    fn a_stop_flag_ends_the_search() {
        let challenge = challenge(30).ceiling(5_000_000);
        assert!(solve(&challenge, &|| true).unwrap().is_none());
    }

    #[test]
    fn a_derived_challenge_still_solves() {
        use crate::kdf::{Derivation, Kdf};

        let challenge = Challenge::new(
            Input::new(b"nonce".to_vec(), Counter::Uint32Be),
            Accept::LeadingZeroBits { bits: 8 },
        )
        .hashed(Hash::Derive {
            derivation: Derivation::new(Kdf::Sha256, b"salt".to_vec(), 1, 32),
        })
        .ceiling(100_000);

        let solution = solve(&challenge, &never()).unwrap().unwrap();
        assert!(challenge.verify(solution.nonce).unwrap());
    }

    #[test]
    fn every_round_of_a_stepping_challenge_is_solved() {
        let base = Challenge::new(
            Input::new(b"round-seed-".to_vec(), Counter::Text),
            Accept::ModulusZero { modulus: 500 },
        )
        .ceiling(500_000);

        let rounds = Rounds::stepping_modulus(base, 4, 500);
        assert_eq!(rounds.rounds.len(), 4);

        let solved = rounds.solve(&never()).unwrap().unwrap();
        assert_eq!(solved.solutions.len(), 4);
        assert_eq!(solved.nonces().len(), 4);
        assert!(solved.total_attempts() > 0);

        for (round, solution) in rounds.rounds.iter().zip(solved.solutions.iter()) {
            assert!(round.verify(solution.nonce).unwrap());
        }
    }

    #[test]
    fn the_modulus_grows_by_one_each_round() {
        let base = Challenge::new(Input::default(), Accept::ModulusZero { modulus: 10 });
        let rounds = Rounds::stepping_modulus(base, 3, 100);

        let moduli: Vec<u32> = rounds
            .rounds
            .iter()
            .map(|round| match round.accept {
                Accept::ModulusZero { modulus } => modulus,
                _ => 0,
            })
            .collect();

        assert_eq!(moduli, vec![100, 101, 102]);
    }
}
