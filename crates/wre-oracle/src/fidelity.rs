use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use wre_core::address::leaves;
use wre_core::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Bucket {
    Match,
    Volatile,
    Gap,
    Missing,
    Extra,
}

impl Bucket {
    pub fn is_failure(self) -> bool {
        matches!(self, Bucket::Gap | Bucket::Missing | Bucket::Extra)
    }

    pub fn describe(self) -> &'static str {
        match self {
            Bucket::Match => "the built payload agrees with every real run",
            Bucket::Volatile => "the real runs disagree with each other, so nothing can be asked of it",
            Bucket::Gap => "the real runs agree and the built payload does not, which is a defect",
            Bucket::Missing => "the real runs carry it and the built payload does not",
            Bucket::Extra => "the built payload carries it and no real run does",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    pub address: String,
    pub bucket: Bucket,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub real: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub built: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Fidelity {
    pub verdicts: Vec<Verdict>,
}

impl Fidelity {
    pub fn passes(&self) -> bool {
        !self.verdicts.iter().any(|verdict| verdict.bucket.is_failure())
    }

    pub fn failures(&self) -> Vec<&Verdict> {
        self.verdicts
            .iter()
            .filter(|verdict| verdict.bucket.is_failure())
            .collect()
    }

    pub fn counts(&self) -> BTreeMap<Bucket, usize> {
        let mut out = BTreeMap::new();
        for verdict in &self.verdicts {
            *out.entry(verdict.bucket).or_insert(0) += 1;
        }
        out
    }

    pub fn summary(&self) -> String {
        let counts = self.counts();
        let read = |bucket: Bucket| counts.get(&bucket).copied().unwrap_or(0);

        format!(
            "{} match, {} volatile, {} gaps, {} missing, {} extra",
            read(Bucket::Match),
            read(Bucket::Volatile),
            read(Bucket::Gap),
            read(Bucket::Missing),
            read(Bucket::Extra)
        )
    }

    pub fn render(&self) -> String {
        let mut out = String::from("| address | verdict | real | built |\n| --- | --- | --- | --- |\n");

        for verdict in &self.verdicts {
            if verdict.bucket == Bucket::Match {
                continue;
            }

            out.push_str(&format!(
                "| {} | {:?} | {} | {} |\n",
                verdict.address,
                verdict.bucket,
                render_value(verdict.real.as_ref()),
                render_value(verdict.built.as_ref())
            ));
        }

        out
    }
}

fn render_value(value: Option<&Value>) -> String {
    match value {
        None => "absent".to_string(),
        Some(value) => {
            let text = value.to_string();
            if text.chars().count() > 40 {
                let clipped: String = text.chars().take(40).collect();
                format!("{clipped}…")
            } else {
                text
            }
        }
    }
}

fn flatten(value: &Value) -> BTreeMap<String, Value> {
    leaves(value)
        .into_iter()
        .map(|(address, leaf)| (address.to_string(), leaf.clone()))
        .filter(|(address, _)| !address.is_empty())
        .collect()
}

pub fn compare(real: &[Value], built: &Value) -> Result<Fidelity> {
    if real.len() < 2 {
        return Err(Error::msg(
            "grading needs at least two real runs, otherwise a volatile field looks like a gap",
        ));
    }

    let observed: Vec<BTreeMap<String, Value>> = real.iter().map(flatten).collect();
    let made = flatten(built);

    let mut addresses: BTreeSet<String> = BTreeSet::new();
    for run in &observed {
        addresses.extend(run.keys().cloned());
    }
    addresses.extend(made.keys().cloned());

    let verdicts = addresses
        .into_iter()
        .map(|address| {
            let seen: Vec<Option<&Value>> =
                observed.iter().map(|run| run.get(&address)).collect();

            let first = seen[0];
            let steady = seen.iter().all(|value| *value == first);
            let anywhere = seen.iter().any(Option::is_some);
            let built_value = made.get(&address);

            let bucket = match (anywhere, built_value) {
                (false, Some(_)) => Bucket::Extra,
                (true, None) => {
                    if steady {
                        Bucket::Missing
                    } else {
                        Bucket::Volatile
                    }
                }
                (true, Some(value)) => {
                    if !steady {
                        Bucket::Volatile
                    } else if first == Some(value) {
                        Bucket::Match
                    } else {
                        Bucket::Gap
                    }
                }
                (false, None) => Bucket::Match,
            };

            Verdict {
                address,
                bucket,
                real: first.cloned(),
                built: built_value.cloned(),
            }
        })
        .collect();

    Ok(Fidelity { verdicts })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn real() -> Vec<Value> {
        vec![
            json!({
                "ua": "Chrome/140",
                "cores": 10,
                "elapsed": 42,
                "screen": { "w": 1728, "h": 1117 }
            }),
            json!({
                "ua": "Chrome/140",
                "cores": 10,
                "elapsed": 51,
                "screen": { "w": 1728, "h": 1117 }
            }),
        ]
    }

    #[test]
    fn a_faithful_payload_passes() {
        let built = json!({
            "ua": "Chrome/140",
            "cores": 10,
            "elapsed": 47,
            "screen": { "w": 1728, "h": 1117 }
        });

        let graded = compare(&real(), &built).unwrap();

        assert!(graded.passes(), "{}", graded.render());
        assert!(graded.summary().contains("0 gaps"));
    }

    #[test]
    fn a_field_that_moves_between_real_runs_is_volatile_not_a_gap() {
        let built = json!({
            "ua": "Chrome/140",
            "cores": 10,
            "elapsed": 999999,
            "screen": { "w": 1728, "h": 1117 }
        });

        let graded = compare(&real(), &built).unwrap();
        let elapsed = graded
            .verdicts
            .iter()
            .find(|verdict| verdict.address.contains("elapsed"))
            .unwrap();

        assert_eq!(elapsed.bucket, Bucket::Volatile);
        assert!(graded.passes());
    }

    #[test]
    fn a_steady_field_that_disagrees_is_a_gap() {
        let built = json!({
            "ua": "Chrome/140",
            "cores": 4,
            "elapsed": 47,
            "screen": { "w": 1728, "h": 1117 }
        });

        let graded = compare(&real(), &built).unwrap();

        assert!(!graded.passes());
        assert_eq!(graded.failures().len(), 1);
        assert_eq!(graded.failures()[0].bucket, Bucket::Gap);
        assert!(graded.failures()[0].address.contains("cores"));
    }

    #[test]
    fn a_field_the_build_forgot_is_missing() {
        let built = json!({ "ua": "Chrome/140", "cores": 10, "elapsed": 47 });

        let graded = compare(&real(), &built).unwrap();
        let missing: Vec<&str> = graded
            .failures()
            .iter()
            .filter(|verdict| verdict.bucket == Bucket::Missing)
            .map(|verdict| verdict.address.as_str())
            .collect();

        assert_eq!(missing.len(), 2, "{missing:?}");
        assert!(missing.iter().all(|address| address.contains("screen")));
    }

    #[test]
    fn a_field_no_real_run_carries_is_extra() {
        let built = json!({
            "ua": "Chrome/140",
            "cores": 10,
            "elapsed": 47,
            "screen": { "w": 1728, "h": 1117 },
            "invented": true
        });

        let graded = compare(&real(), &built).unwrap();
        let failures = graded.failures();
        let extra = failures
            .iter()
            .find(|verdict| verdict.bucket == Bucket::Extra)
            .unwrap();

        assert!(extra.address.contains("invented"));
    }

    #[test]
    fn one_real_run_is_not_enough_to_grade_against() {
        let error = compare(&real()[..1], &json!({})).unwrap_err().to_string();
        assert!(error.contains("at least two real runs"), "{error}");
    }

    #[test]
    fn the_table_leaves_out_the_fields_that_agree() {
        let built = json!({
            "ua": "Chrome/140",
            "cores": 4,
            "elapsed": 47,
            "screen": { "w": 1728, "h": 1117 }
        });

        let table = compare(&real(), &built).unwrap().render();

        assert!(table.contains("cores"));
        assert!(!table.contains("\"Chrome/140\""));
    }

    #[test]
    fn every_bucket_explains_itself() {
        for bucket in [
            Bucket::Match,
            Bucket::Volatile,
            Bucket::Gap,
            Bucket::Missing,
            Bucket::Extra,
        ] {
            assert!(!bucket.describe().is_empty());
        }

        assert!(!Bucket::Match.is_failure());
        assert!(!Bucket::Volatile.is_failure());
        assert!(Bucket::Gap.is_failure());
    }

    #[test]
    fn a_grading_round_trips_through_json() {
        let graded = compare(&real(), &json!({ "ua": "Chrome/140" })).unwrap();
        let text = serde_json::to_string(&graded).unwrap();

        assert_eq!(serde_json::from_str::<Fidelity>(&text).unwrap(), graded);
    }
}
