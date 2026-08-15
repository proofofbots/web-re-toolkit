use std::collections::BTreeSet;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

use crate::stream::xor_repeating;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeriodScore {
    pub period: usize,
    pub coincidence: f64,
}

pub fn coincidence_periods(cipher: &[u8], max_period: usize) -> Vec<PeriodScore> {
    let limit = max_period.min(cipher.len().saturating_sub(1)).max(1);
    let mut out = Vec::with_capacity(limit);

    for period in 1..=limit {
        let mut matches = 0usize;
        let mut compared = 0usize;

        for column in 0..period {
            let bytes: Vec<u8> = cipher.iter().skip(column).step_by(period).copied().collect();
            if bytes.len() < 2 {
                continue;
            }

            let mut counts = [0usize; 256];
            for byte in &bytes {
                counts[*byte as usize] += 1;
            }

            for count in counts {
                matches += count * count.saturating_sub(1);
            }
            compared += bytes.len() * (bytes.len() - 1);
        }

        let coincidence = if compared == 0 {
            0.0
        } else {
            matches as f64 / compared as f64
        };

        out.push(PeriodScore { period, coincidence });
    }

    out.sort_by(|left, right| {
        right
            .coincidence
            .partial_cmp(&left.coincidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.period.cmp(&right.period))
    });

    out
}

pub fn printable_score(bytes: &[u8]) -> f64 {
    if bytes.is_empty() {
        return 0.0;
    }

    let mut score = 0.0;
    for byte in bytes {
        score += match byte {
            b'a'..=b'z' => 3.0,
            b'A'..=b'Z' => 2.0,
            b'0'..=b'9' => 2.0,
            b' ' => 3.0,
            b'\n' | b'\r' | b'\t' => 1.0,
            0x20..=0x7e => 1.0,
            _ => -6.0,
        };
    }

    score / bytes.len() as f64
}

const LETTER_FREQUENCY: [f64; 26] = [
    8.17, 1.49, 2.78, 4.25, 12.70, 2.23, 2.02, 6.09, 6.97, 0.15, 0.77, 4.03, 2.41, 6.75, 7.51,
    1.93, 0.10, 5.99, 6.33, 9.06, 2.76, 0.98, 2.36, 0.15, 1.97, 0.07,
];

fn weight(byte: u8) -> f64 {
    match byte {
        b'a'..=b'z' => LETTER_FREQUENCY[(byte - b'a') as usize].ln(),
        b'A'..=b'Z' => LETTER_FREQUENCY[(byte - b'A') as usize].ln() - 1.0,
        b'0'..=b'9' => 1.0,
        b' ' => 2.0,
        b'\n' | b'\r' | b'\t' => 0.0,
        0x20..=0x7e => -0.5,
        _ => -8.0,
    }
}

pub fn frequency_score(bytes: &[u8]) -> f64 {
    if bytes.is_empty() {
        return 0.0;
    }
    bytes.iter().map(|byte| weight(*byte)).sum::<f64>() / bytes.len() as f64
}

pub fn json_score(bytes: &[u8]) -> f64 {
    if bytes.is_empty() {
        return 0.0;
    }

    let mut bonus = 0.0;
    for byte in bytes {
        bonus += match byte {
            b'"' | b':' | b',' => 2.5,
            b'{' | b'}' | b'[' | b']' => 2.0,
            _ => 0.0,
        };
    }

    frequency_score(bytes) + bonus / bytes.len() as f64
}

pub fn recover_xor_crib(cipher: &[u8], crib: &[u8], period: usize) -> Result<Vec<Vec<u8>>> {
    if crib.is_empty() {
        return Err(Error::msg("a crib must not be empty"));
    }
    if period == 0 {
        return Err(Error::msg("the key period must not be zero"));
    }
    if crib.len() > cipher.len() {
        return Err(Error::msg("the crib is longer than the ciphertext"));
    }

    let mut found: Vec<Vec<u8>> = Vec::new();

    for offset in 0..=cipher.len() - crib.len() {
        let mut key: Vec<Option<u8>> = vec![None; period];
        let mut consistent = true;

        for (index, plain) in crib.iter().enumerate() {
            let slot = (offset + index) % period;
            let byte = cipher[offset + index] ^ plain;

            match key[slot] {
                Some(existing) if existing != byte => {
                    consistent = false;
                    break;
                }
                _ => key[slot] = Some(byte),
            }
        }

        if !consistent {
            continue;
        }

        if let Some(complete) = key.iter().copied().collect::<Option<Vec<u8>>>()
            && !found.contains(&complete)
        {
            found.push(complete);
        }
    }

    Ok(found)
}

pub fn recover_xor_key(cipher: &[u8], period: usize, score: fn(&[u8]) -> f64) -> Result<Vec<u8>> {
    if period == 0 {
        return Err(Error::msg("the key period must not be zero"));
    }
    if cipher.is_empty() {
        return Err(Error::msg("nothing to recover a key from"));
    }

    let mut key = Vec::with_capacity(period);

    for column in 0..period {
        let bytes: Vec<u8> = cipher.iter().skip(column).step_by(period).copied().collect();
        if bytes.is_empty() {
            key.push(0);
            continue;
        }

        let best = (0u16..=255)
            .map(|candidate| {
                let candidate = candidate as u8;
                let plain: Vec<u8> = bytes.iter().map(|byte| byte ^ candidate).collect();
                (candidate, score(&plain))
            })
            .max_by(|left, right| {
                left.1
                    .partial_cmp(&right.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(candidate, _)| candidate)
            .unwrap_or(0);

        key.push(best);
    }

    Ok(key)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recovery {
    pub key: Vec<u8>,
    pub period: usize,
    pub score: f64,
    pub preview: String,
}

pub const KEY_BYTE_COST: f64 = 6.0;

pub fn recover_xor(
    cipher: &[u8],
    max_period: usize,
    score: fn(&[u8]) -> f64,
) -> Result<Vec<Recovery>> {
    recover_xor_with(cipher, max_period, score, 0.85, KEY_BYTE_COST)
}

pub fn recover_xor_with(
    cipher: &[u8],
    max_period: usize,
    score: fn(&[u8]) -> f64,
    band: f64,
    key_byte_cost: f64,
) -> Result<Vec<Recovery>> {
    if cipher.is_empty() {
        return Err(Error::msg("nothing to recover a key from"));
    }

    let ranked = coincidence_periods(cipher, max_period);
    let ceiling = ranked.first().map(|entry| entry.coincidence).unwrap_or(0.0);
    let floor = ceiling * band.clamp(0.0, 1.0);

    let periods: Vec<usize> = ranked
        .iter()
        .filter(|entry| entry.coincidence >= floor)
        .map(|entry| entry.period)
        .collect();

    let mut out = Vec::with_capacity(periods.len());
    for period in periods {
        let key = recover_xor_key(cipher, period, score)?;
        let plain = xor_repeating(cipher, &key)?;
        let penalty = key_byte_cost * period as f64 / cipher.len() as f64;

        out.push(Recovery {
            score: score(&plain) - penalty,
            period,
            key,
            preview: preview(&plain),
        });
    }

    let mut out: Vec<Recovery> = out
        .iter()
        .filter(|candidate| !is_tiled(candidate, &out))
        .cloned()
        .collect();

    out.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.period.cmp(&right.period))
    });

    Ok(out)
}

fn is_tiled(candidate: &Recovery, all: &[Recovery]) -> bool {
    all.iter().any(|shorter| {
        shorter.period < candidate.period
            && candidate.period % shorter.period == 0
            && candidate
                .key
                .iter()
                .enumerate()
                .all(|(index, byte)| *byte == shorter.key[index % shorter.period])
    })
}

fn preview(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(96)
        .map(|byte| if byte.is_ascii_graphic() || *byte == b' ' { *byte as char } else { '.' })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Candidates {
    pub keys: Vec<u64>,
    pub searched: u64,
}

impl Candidates {
    pub fn is_unique(&self) -> bool {
        self.keys.len() == 1
    }

    pub fn only(&self) -> Option<u64> {
        if self.keys.len() == 1 { self.keys.first().copied() } else { None }
    }
}

pub fn search_keyspace<F>(space: std::ops::Range<u64>, accept: F) -> Candidates
where
    F: Fn(u64) -> bool + Send + Sync,
{
    let searched = space.end.saturating_sub(space.start);
    let mut keys: Vec<u64> = space.into_par_iter().filter(|key| accept(*key)).collect();
    keys.sort_unstable();
    Candidates { keys, searched }
}

pub fn intersect(rounds: &[Candidates]) -> Vec<u64> {
    let mut iter = rounds.iter();
    let Some(first) = iter.next() else {
        return Vec::new();
    };

    let mut shared: BTreeSet<u64> = first.keys.iter().copied().collect();
    for round in iter {
        let next: BTreeSet<u64> = round.keys.iter().copied().collect();
        shared = shared.intersection(&next).copied().collect();
        if shared.is_empty() {
            break;
        }
    }

    shared.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"{\"version\":3,\"checks\":[{\"name\":\"webdriver\",\"result\":false},\
{\"name\":\"plugins\",\"result\":true}],\"session\":\"abcdef0123456789\",\"elapsed\":142}";

    #[test]
    fn a_repeating_key_is_recovered_from_ciphertext_alone() {
        let key = b"omgtopkek";
        let cipher = xor_repeating(SAMPLE, key).unwrap();

        let found = recover_xor(&cipher, 24, json_score).unwrap();
        let best = found.first().unwrap();

        assert_eq!(best.key, key);
        assert_eq!(xor_repeating(&cipher, &best.key).unwrap(), SAMPLE);
    }

    #[test]
    fn the_true_period_ranks_highly_on_coincidence() {
        let cipher = xor_repeating(SAMPLE, b"secret!!").unwrap();
        let ranked = coincidence_periods(&cipher, 32);
        let top: Vec<usize> = ranked.iter().take(6).map(|entry| entry.period).collect();
        assert!(top.contains(&8), "period 8 missing from {top:?}");
    }

    #[test]
    fn a_crib_pins_the_key_exactly() {
        let key = b"omgtopkek";
        let cipher = xor_repeating(SAMPLE, key).unwrap();

        let found = recover_xor_crib(&cipher, b"\"webdriver\"", key.len()).unwrap();
        assert_eq!(found, vec![key.to_vec()]);
    }

    #[test]
    fn a_crib_that_is_absent_yields_nothing() {
        let cipher = xor_repeating(SAMPLE, b"omgtopkek").unwrap();
        assert!(recover_xor_crib(&cipher, b"not in the payload", 9).unwrap().is_empty());
    }

    #[test]
    fn a_short_crib_can_leave_the_key_ambiguous() {
        let cipher = xor_repeating(SAMPLE, b"omgtopkek").unwrap();
        let found = recover_xor_crib(&cipher, b"se", 9).unwrap();
        assert!(found.is_empty(), "two bytes cannot fill a nine byte key");
    }

    #[test]
    fn malformed_crib_requests_are_rejected() {
        assert!(recover_xor_crib(b"body", b"", 4).is_err());
        assert!(recover_xor_crib(b"body", b"crib", 0).is_err());
        assert!(recover_xor_crib(b"ab", b"longer", 4).is_err());
    }

    #[test]
    fn the_frequency_model_separates_english_from_a_shifted_copy() {
        let plain = b"the result of the session check was recorded";
        let shifted: Vec<u8> = plain.iter().map(|byte| byte ^ 20).collect();
        assert!(frequency_score(plain) > frequency_score(&shifted));
    }

    #[test]
    fn scoring_prefers_text_over_noise() {
        assert!(printable_score(b"the quick brown fox") > printable_score(&[0u8, 255, 3, 200]));
        assert!(json_score(b"{\"a\":1}") > printable_score(b"{\"a\":1}"));
    }

    #[test]
    fn a_zero_period_is_rejected() {
        assert!(recover_xor_key(b"data", 0, printable_score).is_err());
        assert!(recover_xor_key(b"", 4, printable_score).is_err());
    }

    #[test]
    fn a_keyspace_search_finds_every_accepting_key() {
        let found = search_keyspace(0..4096, |key| key % 1000 == 7);
        assert_eq!(found.keys, vec![7, 1007, 2007, 3007, 4007]);
        assert_eq!(found.searched, 4096);
        assert!(!found.is_unique());
    }

    #[test]
    fn intersecting_rounds_narrows_to_one_key() {
        let first = search_keyspace(0..4096, |key| key % 3 == 1);
        let second = search_keyspace(0..4096, |key| key % 1365 == 1);

        let shared = intersect(&[first, second]);
        assert_eq!(shared, vec![1, 1366, 2731]);
        assert!(intersect(&[]).is_empty());
    }

    #[test]
    fn a_single_candidate_is_reported_as_unique() {
        let found = search_keyspace(0..1024, |key| key == 512);
        assert!(found.is_unique());
        assert_eq!(found.only(), Some(512));
    }
}
