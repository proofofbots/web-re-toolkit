use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};
use wre_pow::input::{Counter, Input};
use wre_pow::predicate::Accept;
use wre_pow::search::{Challenge as Search, Solution, solve};

pub const ROUNDS: usize = 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Challenge {
    pub id: u32,
    pub token: String,
    pub salt: String,
    pub difficulty: u32,
    pub delay: u32,
    pub slice: u32,
    pub version: u32,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub prefix: String,
    pub nonces: Vec<String>,
    pub attempts: Vec<u64>,
    pub elapsed_ms: Vec<u64>,
    pub digests: Vec<String>,
    pub formatted: String,
}

pub fn from_abck(value: &str) -> Vec<Challenge> {
    let decoded = crate::cookies::parse_abck(value);
    let token = decoded.token.clone();

    let mut out: Vec<Challenge> = decoded
        .challenges
        .iter()
        .filter_map(|entry| parse_item(&token, entry))
        .collect();

    out.sort_by_key(|challenge| if challenge.version == 2 { 0 } else { 1 });
    out
}

pub fn parse_item(token: &str, entry: &str) -> Option<Challenge> {
    let parts: Vec<&str> = entry.split('-').collect();
    if parts.len() < 5 {
        return None;
    }

    Some(Challenge {
        id: parts[0].parse().unwrap_or_default(),
        token: token.to_string(),
        salt: parts[1].to_string(),
        difficulty: parts[2].parse().unwrap_or_default(),
        delay: parts[3].parse().unwrap_or_default(),
        slice: parts[4].parse().unwrap_or_default(),
        version: parts.get(5).and_then(|part| part.parse().ok()).unwrap_or(1),
        raw: entry.to_string(),
    })
}

pub fn parse_challenge(text: &str) -> Option<Challenge> {
    let parts: Vec<&str> = text.split('-').collect();
    if parts.len() < 6 {
        return None;
    }

    Some(Challenge {
        id: parts[0].parse().unwrap_or_default(),
        token: parts[1].to_string(),
        salt: parts[2].to_string(),
        difficulty: parts[3].parse().unwrap_or_default(),
        delay: parts[4].parse().unwrap_or_default(),
        slice: parts[5].parse().unwrap_or_default(),
        version: parts.get(6).and_then(|part| part.parse().ok()).unwrap_or(1),
        raw: text.to_string(),
    })
}

pub fn prefix(challenge: &Challenge, start_ts: u64) -> String {
    format!("{}{start_ts}{}", challenge.token, challenge.salt)
}

pub fn solve_rounds(
    challenge: &Challenge,
    start_ts: u64,
    rounds: usize,
    ceiling: u64,
    workers: usize,
) -> Result<Answer> {
    if challenge.difficulty == 0 {
        return Err(Error::msg("the challenge carries no difficulty"));
    }

    let head = prefix(challenge, start_ts);
    let mut solutions: Vec<(u32, Solution)> = Vec::with_capacity(rounds);

    for round in 0..rounds {
        let modulus = challenge.difficulty.saturating_add(round as u32);
        let search = Search::new(
            Input::new(format!("{head}{modulus}").into_bytes(), Counter::HexLower),
            Accept::ModulusZero { modulus },
        )
        .workers(workers.clamp(1, 64))
        .ceiling(ceiling);

        let Some(solution) = solve(&search, &|| false)? else {
            return Err(Error::msg(format!(
                "round {round} found no nonce below {ceiling} at modulus {modulus}"
            )));
        };

        solutions.push((modulus, solution));
    }

    let nonces: Vec<String> = solutions
        .iter()
        .map(|(_, solution)| format!("{:x}", solution.nonce))
        .collect();
    let attempts: Vec<u64> = solutions.iter().map(|(_, solution)| solution.attempts).collect();
    let elapsed: Vec<u64> = solutions.iter().map(|(_, solution)| solution.elapsed_ms).collect();
    let digests: Vec<String> = solutions
        .iter()
        .map(|(_, solution)| solution.digest_hex())
        .collect();

    let trace = match solutions.first() {
        Some((modulus, solution)) => vec![
            challenge.salt.clone(),
            start_ts.to_string(),
            challenge.raw.clone(),
            head.clone(),
            challenge.difficulty.to_string(),
            modulus.to_string(),
            nonces[0].clone(),
            format!("{head}{modulus}{}", nonces[0]),
            solution.digest_hex(),
            solution.elapsed_ms.to_string(),
        ],
        None => Vec::new(),
    };

    let formatted = format!(
        "{};{};{};{};",
        nonces.join(","),
        elapsed.iter().map(u64::to_string).collect::<Vec<_>>().join(","),
        attempts.iter().map(u64::to_string).collect::<Vec<_>>().join(","),
        trace.join(",")
    );

    Ok(Answer { prefix: head, nonces, attempts, elapsed_ms: elapsed, digests, formatted })
}

pub fn verify(challenge: &Challenge, start_ts: u64, nonces: &[String]) -> Result<bool> {
    let head = prefix(challenge, start_ts);

    for (round, nonce) in nonces.iter().enumerate() {
        let modulus = challenge.difficulty.saturating_add(round as u32);
        let digest = wre_pow::hash::Hash::Sha256.digest(format!("{head}{modulus}{nonce}").as_bytes())?;

        if wre_pow::predicate::remainder(&digest, modulus) != 0 {
            return Ok(false);
        }
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COOKIE: &str = "TOKEN~-1~salt~-1~3-abcd-500-1000-30-2||4-efgh-700-1000-30";

    #[test]
    fn the_work_items_come_out_of_the_cookie_with_version_two_first() {
        let challenges = from_abck(COOKIE);

        assert_eq!(challenges.len(), 2);
        assert_eq!(challenges[0].version, 2);
        assert_eq!(challenges[0].salt, "abcd");
        assert_eq!(challenges[0].difficulty, 500);
        assert_eq!(challenges[0].token, "TOKEN");
        assert_eq!(challenges[1].difficulty, 700);
    }

    #[test]
    fn a_cookie_with_no_work_yields_no_challenges() {
        assert!(from_abck("TOKEN~-1~salt~-1~-1").is_empty());
    }

    #[test]
    fn ten_rounds_solve_and_verify() {
        let challenge = from_abck(COOKIE).remove(0);
        let answer = solve_rounds(&challenge, 1_760_000_000_000, ROUNDS, 5_000_000, 4).unwrap();

        assert_eq!(answer.nonces.len(), ROUNDS);
        assert_eq!(answer.prefix, "TOKEN1760000000000abcd");
        assert!(verify(&challenge, 1_760_000_000_000, &answer.nonces).unwrap());

        let parts: Vec<&str> = answer.formatted.split(';').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].split(',').count(), ROUNDS);
        assert_eq!(parts[2].split(',').count(), ROUNDS);
        assert!(parts[3].starts_with("abcd,1760000000000,3-abcd-500-1000-30-2,"));
    }

    #[test]
    fn a_wrong_nonce_does_not_verify() {
        let challenge = from_abck(COOKIE).remove(0);
        let nonces = vec!["1".to_string(); ROUNDS];

        assert!(!verify(&challenge, 1_760_000_000_000, &nonces).unwrap());
    }

    #[test]
    fn a_challenge_string_parses_on_its_own() {
        let challenge = parse_challenge("3-TOKEN-salt-500-1000-30-2").expect("challenge");

        assert_eq!(challenge.token, "TOKEN");
        assert_eq!(challenge.salt, "salt");
        assert_eq!(challenge.difficulty, 500);
        assert_eq!(challenge.version, 2);
    }
}
