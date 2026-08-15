use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use wre_core::address::Address;
use wre_core::error::Result;
use wre_wire::payload::{Change, FieldDiff, diff};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Knob {
    pub name: String,
    pub group: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

impl Knob {
    pub fn new(name: &str, group: &str) -> Self {
        Self {
            name: name.to_string(),
            group: group.to_string(),
            note: None,
            payload: Value::Null,
        }
    }

    pub fn with_payload(mut self, payload: Value) -> Self {
        self.payload = payload;
        self
    }

    pub fn with_note(mut self, note: &str) -> Self {
        self.note = Some(note.to_string());
        self
    }
}

#[derive(Debug, Clone)]
pub struct SweepOptions {
    pub baseline_runs: usize,
    pub repeats: usize,
    pub keep_noise: bool,
    pub stop_on_error: bool,
}

impl Default for SweepOptions {
    fn default() -> Self {
        Self {
            baseline_runs: 2,
            repeats: 1,
            keep_noise: false,
            stop_on_error: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmResult {
    pub knob: String,
    pub group: String,
    #[serde(default)]
    pub attributed: Vec<Address>,
    #[serde(default)]
    pub moved: Vec<FieldDiff>,
    #[serde(default)]
    pub suppressed: Vec<Address>,
    #[serde(default)]
    pub error: Option<String>,
}

impl ArmResult {
    pub fn is_silent(&self) -> bool {
        self.error.is_none() && self.attributed.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweepReport {
    pub baseline: Value,
    pub noise: Vec<Address>,
    pub arms: Vec<ArmResult>,
    pub baseline_runs: usize,
}

impl SweepReport {
    pub fn signal_map(&self) -> BTreeMap<Address, Vec<String>> {
        let mut out: BTreeMap<Address, Vec<String>> = BTreeMap::new();

        for arm in &self.arms {
            for address in &arm.attributed {
                out.entry(address.clone()).or_default().push(arm.knob.clone());
            }
        }

        out
    }

    pub fn by_group(&self) -> BTreeMap<String, Vec<&ArmResult>> {
        let mut out: BTreeMap<String, Vec<&ArmResult>> = BTreeMap::new();
        for arm in &self.arms {
            out.entry(arm.group.clone()).or_default().push(arm);
        }
        out
    }

    pub fn silent_arms(&self) -> Vec<&ArmResult> {
        self.arms.iter().filter(|arm| arm.is_silent()).collect()
    }

    pub fn failed_arms(&self) -> Vec<&ArmResult> {
        self.arms.iter().filter(|arm| arm.error.is_some()).collect()
    }

    pub fn exclusive_addresses(&self) -> BTreeMap<Address, String> {
        self.signal_map()
            .into_iter()
            .filter(|(_, knobs)| knobs.len() == 1)
            .map(|(address, knobs)| (address, knobs[0].clone()))
            .collect()
    }

    pub fn summary(&self) -> String {
        let map = self.signal_map();
        format!(
            "{} arms, {} addresses attributed, {} exclusive, {} noisy addresses, {} silent, {} failed",
            self.arms.len(),
            map.len(),
            self.exclusive_addresses().len(),
            self.noise.len(),
            self.silent_arms().len(),
            self.failed_arms().len()
        )
    }
}

pub fn noise_floor(samples: &[Value]) -> BTreeSet<Address> {
    let mut out = BTreeSet::new();

    if samples.len() < 2 {
        return out;
    }

    for window in samples.windows(2) {
        for entry in diff(&window[0], &window[1]) {
            out.insert(entry.address);
        }
    }

    out
}

pub fn sweep<F>(knobs: &[Knob], mut run: F, options: SweepOptions) -> Result<SweepReport>
where
    F: FnMut(Option<&Knob>) -> Result<Value>,
{
    let baseline_runs = options.baseline_runs.max(1);
    let mut baselines = Vec::with_capacity(baseline_runs);

    for _ in 0..baseline_runs {
        baselines.push(run(None)?);
    }

    let noise = noise_floor(&baselines);
    let baseline = baselines[0].clone();
    let mut arms = Vec::with_capacity(knobs.len());

    for knob in knobs {
        let mut attributed: BTreeSet<Address> = BTreeSet::new();
        let mut suppressed: BTreeSet<Address> = BTreeSet::new();
        let mut moved: Vec<FieldDiff> = Vec::new();
        let mut error = None;

        for _ in 0..options.repeats.max(1) {
            match run(Some(knob)) {
                Ok(observed) => {
                    for entry in diff(&baseline, &observed) {
                        if noise.contains(&entry.address) && !options.keep_noise {
                            suppressed.insert(entry.address.clone());
                            continue;
                        }
                        attributed.insert(entry.address.clone());
                        moved.push(entry);
                    }
                }
                Err(failure) => {
                    error = Some(failure.to_string());
                    if options.stop_on_error {
                        break;
                    }
                }
            }
        }

        moved.sort_by(|left, right| left.address.cmp(&right.address));
        moved.dedup_by(|left, right| left.address == right.address && left.change == right.change);

        arms.push(ArmResult {
            knob: knob.name.clone(),
            group: knob.group.clone(),
            attributed: attributed.into_iter().collect(),
            moved,
            suppressed: suppressed.into_iter().collect(),
            error,
        });

        if options.stop_on_error && arms.last().is_some_and(|arm| arm.error.is_some()) {
            break;
        }
    }

    Ok(SweepReport {
        baseline,
        noise: noise.into_iter().collect(),
        arms,
        baseline_runs,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionArm {
    pub knob: String,
    pub verdict: Value,
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionReport {
    pub reference: Value,
    pub arms: Vec<AttributionArm>,
    pub budget_used: usize,
}

impl AttributionReport {
    pub fn movers(&self) -> Vec<&AttributionArm> {
        self.arms.iter().filter(|arm| arm.changed).collect()
    }
}

pub fn attribute<F>(
    knobs: &[Knob],
    mut score: F,
    budget: usize,
) -> Result<AttributionReport>
where
    F: FnMut(Option<&Knob>) -> Result<Value>,
{
    let reference = score(None)?;
    let mut arms = Vec::new();
    let mut used = 1usize;

    for knob in knobs {
        if budget > 0 && used >= budget {
            break;
        }

        let verdict = score(Some(knob))?;
        used += 1;

        arms.push(AttributionArm {
            knob: knob.name.clone(),
            changed: verdict != reference,
            verdict,
        });
    }

    Ok(AttributionReport { reference, arms, budget_used: used })
}

pub fn render_signal_map(report: &SweepReport) -> String {
    let mut out = String::from("| address | moved by |\n| --- | --- |\n");

    for (address, knobs) in report.signal_map() {
        out.push_str(&format!("| `{address}` | {} |\n", knobs.join(", ")));
    }

    out
}

pub fn render_arms(report: &SweepReport) -> String {
    let mut out = String::from("| knob | group | addresses | note |\n| --- | --- | --- | --- |\n");

    for arm in &report.arms {
        let note = match &arm.error {
            Some(error) => format!("failed: {error}"),
            None if arm.attributed.is_empty() => "no address moved".to_string(),
            None => String::new(),
        };

        out.push_str(&format!(
            "| {} | {} | {} | {note} |\n",
            arm.knob,
            arm.group,
            arm.attributed.len()
        ));
    }

    out
}

pub fn changed_only(diffs: &[FieldDiff]) -> Vec<&FieldDiff> {
    diffs
        .iter()
        .filter(|entry| entry.change == Change::Changed)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn subtracts_the_noise_floor() {
        let knobs = vec![Knob::new("screen", "display"), Knob::new("timezone", "locale")];
        let mut call = 0usize;

        let report = sweep(
            &knobs,
            |knob| {
                call += 1;
                Ok(match knob.map(|entry| entry.name.as_str()) {
                    None => json!({ "screen": 1512, "tz": "UTC", "nonce": call }),
                    Some("screen") => json!({ "screen": 1920, "tz": "UTC", "nonce": call }),
                    Some(_) => json!({ "screen": 1512, "tz": "Europe/Berlin", "nonce": call }),
                })
            },
            SweepOptions::default(),
        )
        .unwrap();

        assert_eq!(report.noise, vec![Address::parse("nonce").unwrap()]);

        let screen = &report.arms[0];
        assert_eq!(screen.attributed, vec![Address::parse("screen").unwrap()]);
        assert_eq!(screen.suppressed, vec![Address::parse("nonce").unwrap()]);

        let timezone = &report.arms[1];
        assert_eq!(timezone.attributed, vec![Address::parse("tz").unwrap()]);
    }

    #[test]
    fn builds_an_inverted_signal_map() {
        let knobs = vec![Knob::new("a", "g"), Knob::new("b", "g")];

        let report = sweep(
            &knobs,
            |knob| {
                Ok(match knob.map(|entry| entry.name.as_str()) {
                    None => json!({ "shared": 0, "only_a": 0 }),
                    Some("a") => json!({ "shared": 1, "only_a": 1 }),
                    Some(_) => json!({ "shared": 1, "only_a": 0 }),
                })
            },
            SweepOptions::default(),
        )
        .unwrap();

        let map = report.signal_map();
        assert_eq!(map.get(&Address::parse("shared").unwrap()).unwrap().len(), 2);
        assert_eq!(report.exclusive_addresses().len(), 1);
        assert!(render_signal_map(&report).contains("only_a"));
    }

    #[test]
    fn records_silent_and_failed_arms() {
        let knobs = vec![Knob::new("quiet", "g"), Knob::new("broken", "g")];

        let report = sweep(
            &knobs,
            |knob| match knob.map(|entry| entry.name.as_str()) {
                Some("broken") => Err(wre_core::error::Error::msg("navigation failed")),
                _ => Ok(json!({ "same": 1 })),
            },
            SweepOptions::default(),
        )
        .unwrap();

        assert_eq!(report.silent_arms().len(), 1);
        assert_eq!(report.failed_arms().len(), 1);
        assert!(report.summary().contains("1 failed"));
    }

    #[test]
    fn attributes_against_a_verdict_oracle() {
        let knobs = vec![Knob::new("ua", "agent"), Knob::new("tz", "locale")];

        let report = attribute(
            &knobs,
            |knob| {
                Ok(match knob.map(|entry| entry.name.as_str()) {
                    Some("ua") => json!({ "score": 90 }),
                    _ => json!({ "score": 10 }),
                })
            },
            0,
        )
        .unwrap();

        assert_eq!(report.movers().len(), 1);
        assert_eq!(report.movers()[0].knob, "ua");
        assert_eq!(report.budget_used, 3);
    }

    #[test]
    fn a_budget_stops_the_sweep_early() {
        let knobs: Vec<Knob> = (0..10).map(|index| Knob::new(&format!("k{index}"), "g")).collect();
        let report = attribute(&knobs, |_| Ok(json!(1)), 4).unwrap();
        assert_eq!(report.budget_used, 4);
        assert_eq!(report.arms.len(), 3);
    }
}
