use std::time::Instant;

use serde::{Deserialize, Serialize};
use wre_pow::hash::sha256_hex;
use wre_pow::predicate::score;

pub const DIFFICULTY: f64 = 10.0;
pub const COUNT: usize = 2;

const NIBBLES: usize = 13;
const CEILING: u64 = 10_000_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proof {
    #[serde(rename = "workTime")]
    pub work_time: i64,
    pub id: String,
    pub answers: Vec<u64>,
    pub duration: f64,
    pub d: i64,
    pub st: i64,
    pub rst: i64,
}

impl Proof {
    pub fn header(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub ct: String,
    pub salt: String,
    pub id: String,
    pub work_time: i64,
    pub difficulty: f64,
    pub count: usize,
    pub extra: String,
    pub st: i64,
    pub rst: i64,
}

pub fn seed(ct: &str, work_time: i64, id: &str, salt: &str, extra: &str) -> String {
    let prefix: String = ct.chars().take(16).collect();
    let mut text = format!("tp-v2-input{prefix}, {work_time}, {id}");

    if !salt.is_empty() {
        text.push_str(&format!(", {salt}"));
    }

    if !extra.is_empty() {
        text.push_str(&format!(", {extra}"));
    }

    sha256_hex(text.as_bytes())
}

pub fn answers(seed: &str, difficulty: f64, count: usize) -> Option<(Vec<u64>, String)> {
    let target = difficulty / count as f64;
    let mut current = seed.to_string();
    let mut found = Vec::with_capacity(count);

    for _ in 0..count {
        let mut candidate = 1u64;

        loop {
            if candidate > CEILING {
                return None;
            }

            let digest = sha256_hex(format!("{candidate}, {current}").as_bytes());
            let bytes = hex::decode(&digest).ok()?;

            if score(&bytes, NIBBLES) >= target {
                found.push(candidate);
                current = digest;
                break;
            }

            candidate += 1;
        }
    }

    Some((found, current))
}

pub fn build(request: &Request) -> Option<Proof> {
    let started = Instant::now();
    let chain = seed(
        &request.ct,
        request.work_time,
        &request.id,
        &request.salt,
        &request.extra,
    );
    let (answers, _) = answers(&chain, request.difficulty, request.count)?;
    let duration = (started.elapsed().as_nanos() as f64 / 1e6 * 1000.0).round() / 1000.0;

    Some(Proof {
        work_time: request.work_time,
        id: request.id.clone(),
        answers,
        duration,
        d: request.d(),
        st: request.st,
        rst: request.rst,
    })
}

impl Request {
    pub fn d(&self) -> i64 {
        if self.st != 0 && self.rst != 0 {
            self.rst - self.st
        } else {
            0
        }
    }
}

pub fn verify(proof: &Proof, ct: &str, salt: &str, difficulty: f64, count: usize) -> bool {
    let chain = seed(ct, proof.work_time, &proof.id, salt, "");
    matches!(answers(&chain, difficulty, count), Some((found, _)) if found == proof.answers)
}

pub fn salts_in(pool: &str) -> Vec<String> {
    let bytes = pool.as_bytes();
    let mut found: Vec<String> = Vec::new();
    let mut run = 0usize;

    for index in 0..bytes.len() {
        if bytes[index].is_ascii_hexdigit() && !bytes[index].is_ascii_uppercase() {
            run += 1;
        } else {
            run = 0;
            continue;
        }

        if run < 64 {
            continue;
        }

        let candidate = pool[index + 1 - 64..=index].to_string();
        if !found.contains(&candidate) {
            found.push(candidate);
        }

        run = 0;
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const CT: &str = "3;1786799692150;abcdefghijklmnopqrstuvwxyz0123456789";
    const SALT: &str = "0f44a7cde3661c88ea0675ee045d307720919bc2a20d6b7d777aea1738f69a9a";

    #[test]
    fn the_seed_reads_the_first_sixteen_characters_of_the_token() {
        let text = seed(CT, 1, "id", "", "");
        let expected = sha256_hex(b"tp-v2-input3;1786799692150;, 1, id");
        assert_eq!(text, expected);
        assert_ne!(text, seed(&CT.replace('3', "4"), 1, "id", "", ""));
    }

    #[test]
    fn every_answer_scores_at_or_above_the_target() {
        let chain = seed(CT, 1_786_799_692_320, "49a8b3e1470b4e5f", SALT, "");
        let (found, _) = answers(&chain, DIFFICULTY, COUNT).expect("no answers");

        assert_eq!(found.len(), COUNT);

        let mut current = chain;
        for answer in &found {
            let digest = sha256_hex(format!("{answer}, {current}").as_bytes());
            let bytes = hex::decode(&digest).unwrap();
            assert!(score(&bytes, NIBBLES) >= DIFFICULTY / COUNT as f64);
            current = digest;
        }
    }

    #[test]
    fn no_smaller_counter_would_have_been_accepted() {
        let chain = seed(CT, 42, "id", SALT, "");
        let (found, _) = answers(&chain, DIFFICULTY, COUNT).unwrap();

        let mut current = chain;
        for answer in &found {
            for candidate in 1..*answer {
                let digest = sha256_hex(format!("{candidate}, {current}").as_bytes());
                let bytes = hex::decode(&digest).unwrap();
                assert!(score(&bytes, NIBBLES) < DIFFICULTY / COUNT as f64);
            }
            current = sha256_hex(format!("{answer}, {current}").as_bytes());
        }
    }

    #[test]
    fn a_built_header_verifies_against_its_own_token() {
        let request = Request {
            ct: CT.to_string(),
            salt: SALT.to_string(),
            id: "49a8b3e1470b4e5f9f1695c579350a91".to_string(),
            work_time: 1_786_799_692_320,
            difficulty: DIFFICULTY,
            count: COUNT,
            extra: String::new(),
            st: 1_786_799_692_150,
            rst: 1_786_799_692_297,
        };

        let proof = build(&request).expect("no proof");
        assert_eq!(proof.d, 147);
        assert!(verify(&proof, CT, SALT, DIFFICULTY, COUNT));
        assert!(!verify(&proof, "another-token", SALT, DIFFICULTY, COUNT));

        let text = proof.header();
        assert!(text.starts_with("{\"workTime\":"));
        assert_eq!(serde_json::from_str::<Proof>(&text).unwrap(), proof);
    }

    #[test]
    fn salts_are_the_lowercase_sixty_four_character_hex_runs() {
        let pool = format!("prefix,{SALT},{SALT},tail0123");
        assert_eq!(salts_in(&pool), vec![SALT.to_string()]);
        assert!(salts_in("0123456789").is_empty());
        assert!(salts_in(&SALT.to_uppercase()).is_empty());
    }
}
