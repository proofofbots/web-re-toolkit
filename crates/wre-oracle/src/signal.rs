use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use wre_core::address::leaves;
use wre_core::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Arm {
    Sound,
    Broken,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trial {
    pub arm: Arm,
    pub observed: Value,
    #[serde(default)]
    pub posts: usize,
    #[serde(default)]
    pub note: String,
}

impl Trial {
    pub fn new(arm: Arm, observed: Value, posts: usize) -> Self {
        Self { arm, observed, posts, note: String::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub address: String,
    pub sound: Vec<Value>,
    pub broken: Vec<Value>,
    pub separated_at: Vec<usize>,
}

impl Candidate {
    pub fn describe(&self) -> String {
        format!(
            "{} tells the two arms apart at post counts {:?}: sound reads {:?}, broken reads {:?}",
            self.address, self.separated_at, self.sound, self.broken
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub candidates: Vec<Candidate>,
    pub compared_at: Vec<usize>,
    pub skipped: Vec<usize>,
}

impl Report {
    pub fn best(&self) -> Option<&Candidate> {
        self.candidates.first()
    }

    pub fn found_one(&self) -> bool {
        !self.candidates.is_empty()
    }

    pub fn summary(&self) -> String {
        if self.compared_at.is_empty() {
            return "no post count carried both a sound and a broken arm, so nothing was compared"
                .to_string();
        }

        format!(
            "{} candidate signals from post counts {:?}, {} counts skipped for having only one arm",
            self.candidates.len(),
            self.compared_at,
            self.skipped.len()
        )
    }
}

fn flatten(value: &Value) -> BTreeMap<String, Value> {
    leaves(value)
        .into_iter()
        .map(|(address, leaf)| (address.to_string(), leaf.clone()))
        .filter(|(address, _)| !address.is_empty())
        .collect()
}

pub fn find_signal(trials: &[Trial]) -> Result<Report> {
    if trials.is_empty() {
        return Err(Error::msg("no trials to look at"));
    }

    let mut grouped: BTreeMap<usize, Vec<&Trial>> = BTreeMap::new();
    for trial in trials {
        grouped.entry(trial.posts).or_default().push(trial);
    }

    let mut compared_at = Vec::new();
    let mut skipped = Vec::new();
    let mut separating: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>, Vec<usize>)> =
        BTreeMap::new();
    let mut ruled_out: BTreeSet<String> = BTreeSet::new();

    for (posts, group) in &grouped {
        let sound: Vec<BTreeMap<String, Value>> = group
            .iter()
            .filter(|trial| trial.arm == Arm::Sound)
            .map(|trial| flatten(&trial.observed))
            .collect();

        let broken: Vec<BTreeMap<String, Value>> = group
            .iter()
            .filter(|trial| trial.arm == Arm::Broken)
            .map(|trial| flatten(&trial.observed))
            .collect();

        if sound.is_empty() || broken.is_empty() {
            skipped.push(*posts);
            continue;
        }

        compared_at.push(*posts);

        let mut addresses: BTreeSet<String> = BTreeSet::new();
        for run in sound.iter().chain(broken.iter()) {
            addresses.extend(run.keys().cloned());
        }

        for address in addresses {
            let sound_values: BTreeSet<String> = sound
                .iter()
                .map(|run| render(run.get(&address)))
                .collect();
            let broken_values: BTreeSet<String> = broken
                .iter()
                .map(|run| render(run.get(&address)))
                .collect();

            if sound_values.is_disjoint(&broken_values) {
                let entry = separating.entry(address).or_insert_with(|| {
                    (BTreeSet::new(), BTreeSet::new(), Vec::new())
                });
                entry.0.extend(sound_values);
                entry.1.extend(broken_values);
                entry.2.push(*posts);
            } else {
                ruled_out.insert(address);
            }
        }
    }

    let mut candidates: Vec<Candidate> = separating
        .into_iter()
        .filter(|(address, _)| !ruled_out.contains(address))
        .map(|(address, (sound, broken, at))| Candidate {
            address,
            sound: sound.iter().map(|text| parse(text)).collect(),
            broken: broken.iter().map(|text| parse(text)).collect(),
            separated_at: at,
        })
        .collect();

    candidates.sort_by(|left, right| {
        right
            .separated_at
            .len()
            .cmp(&left.separated_at.len())
            .then(left.address.cmp(&right.address))
    });

    Ok(Report { candidates, compared_at, skipped })
}

fn render(value: Option<&Value>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "\u{0}absent".to_string(),
    }
}

fn parse(text: &str) -> Value {
    if text == "\u{0}absent" {
        return Value::Null;
    }
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn trial(arm: Arm, cookie_len: usize, status: u16, posts: usize) -> Trial {
        Trial::new(
            arm,
            json!({
                "status": status,
                "cookie": { "length": cookie_len },
                "served_at": format!("run-{posts}-{cookie_len}")
            }),
            posts,
        )
    }

    #[test]
    fn the_field_that_separates_the_arms_is_found() {
        let trials = vec![
            trial(Arm::Sound, 560, 201, 1),
            trial(Arm::Sound, 560, 201, 1),
            trial(Arm::Broken, 552, 201, 1),
            trial(Arm::Broken, 552, 201, 1),
        ];

        let report = find_signal(&trials).unwrap();

        assert!(report.found_one());
        assert_eq!(report.best().unwrap().address, "cookie.length");
        assert!(report.best().unwrap().describe().contains("cookie.length"));
    }

    #[test]
    fn a_field_that_is_the_same_in_both_arms_is_not_a_signal() {
        let trials = vec![
            trial(Arm::Sound, 560, 201, 1),
            trial(Arm::Broken, 552, 201, 1),
        ];

        let report = find_signal(&trials).unwrap();
        let addresses: Vec<&str> = report
            .candidates
            .iter()
            .map(|candidate| candidate.address.as_str())
            .collect();

        assert!(!addresses.contains(&"status"), "{addresses:?}");
    }

    #[test]
    fn arms_at_different_post_counts_are_never_compared() {
        let trials = vec![
            trial(Arm::Sound, 560, 201, 1),
            trial(Arm::Broken, 552, 201, 3),
        ];

        let report = find_signal(&trials).unwrap();

        assert!(!report.found_one());
        assert!(report.compared_at.is_empty());
        assert_eq!(report.skipped, vec![1, 3]);
        assert!(report.summary().contains("nothing was compared"));
    }

    #[test]
    fn a_field_that_only_separates_at_one_post_count_is_dropped() {
        let trials = vec![
            trial(Arm::Sound, 560, 201, 1),
            trial(Arm::Broken, 552, 201, 1),
            trial(Arm::Sound, 900, 201, 2),
            trial(Arm::Broken, 900, 201, 2),
        ];

        let report = find_signal(&trials).unwrap();
        let addresses: Vec<&str> = report
            .candidates
            .iter()
            .map(|candidate| candidate.address.as_str())
            .collect();

        assert!(
            !addresses.contains(&"cookie.length"),
            "a field that agrees at one count is not a signal: {addresses:?}"
        );
    }

    #[test]
    fn a_field_that_separates_everywhere_ranks_first() {
        let trials = vec![
            trial(Arm::Sound, 560, 201, 1),
            trial(Arm::Broken, 552, 403, 1),
            trial(Arm::Sound, 570, 201, 2),
            trial(Arm::Broken, 562, 403, 2),
        ];

        let report = find_signal(&trials).unwrap();

        assert!(report.found_one());
        assert_eq!(report.best().unwrap().separated_at, vec![1, 2]);
        assert_eq!(report.compared_at, vec![1, 2]);
    }

    #[test]
    fn a_field_present_in_one_arm_only_counts_as_separating() {
        let trials = vec![
            Trial::new(Arm::Sound, json!({ "token": "abc" }), 1),
            Trial::new(Arm::Broken, json!({}), 1),
        ];

        let report = find_signal(&trials).unwrap();

        assert_eq!(report.best().unwrap().address, "token");
        assert!(report.best().unwrap().broken.contains(&Value::Null));
    }

    #[test]
    fn no_trials_at_all_is_rejected() {
        assert!(find_signal(&[]).is_err());
    }

    #[test]
    fn a_report_round_trips_through_json() {
        let trials = vec![
            trial(Arm::Sound, 560, 201, 1),
            trial(Arm::Broken, 552, 201, 1),
        ];

        let report = find_signal(&trials).unwrap();
        let text = serde_json::to_string(&report).unwrap();

        assert_eq!(serde_json::from_str::<Report>(&text).unwrap(), report);
    }
}
