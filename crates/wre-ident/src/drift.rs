use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

use crate::locate::Resolution;
use crate::shape::{FunctionShape, ShapeIndex, overlap};

pub const GRAM_WIDTH: usize = 3;
pub const SIGNATURE_WIDTH: usize = 64;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub values: Vec<(u64, usize)>,
}

impl Signature {
    pub fn of(function: &FunctionShape) -> Self {
        Self::from_grams(function.shape.grams(GRAM_WIDTH))
    }

    pub fn from_grams(grams: BTreeMap<u64, usize>) -> Self {
        Self { values: grams.into_iter().take(SIGNATURE_WIDTH).collect() }
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    fn counts(&self) -> BTreeMap<u64, usize> {
        self.values.iter().copied().collect()
    }

    pub fn estimate(&self, other: &Signature) -> f64 {
        if self.values.is_empty() && other.values.is_empty() {
            return 1.0;
        }
        if self.values.is_empty() || other.values.is_empty() {
            return 0.0;
        }

        overlap(&self.counts(), &other.counts())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Identical,
    Renamed,
    Edited,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pair {
    pub before: String,
    pub after: String,
    pub verdict: Verdict,
    pub similarity: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BuildDiff {
    pub pairs: Vec<Pair>,
    pub gone: Vec<String>,
    pub added: Vec<String>,
}

impl BuildDiff {
    pub fn renamed(&self) -> BTreeMap<String, String> {
        self.pairs
            .iter()
            .filter(|pair| pair.verdict != Verdict::Identical)
            .map(|pair| (pair.before.clone(), pair.after.clone()))
            .collect()
    }

    pub fn edited(&self) -> Vec<&Pair> {
        self.pairs
            .iter()
            .filter(|pair| pair.verdict == Verdict::Edited)
            .collect()
    }

    pub fn follow(&self, name: &str) -> Option<&Pair> {
        self.pairs.iter().find(|pair| pair.before == name)
    }

    pub fn summary(&self) -> String {
        let identical = self
            .pairs
            .iter()
            .filter(|pair| pair.verdict == Verdict::Identical)
            .count();
        let renamed = self
            .pairs
            .iter()
            .filter(|pair| pair.verdict == Verdict::Renamed)
            .count();

        format!(
            "{identical} unchanged, {renamed} renamed, {} edited, {} gone, {} new",
            self.edited().len(),
            self.gone.len(),
            self.added.len()
        )
    }
}

pub fn compare(before: &ShapeIndex, after: &ShapeIndex, threshold: f64) -> BuildDiff {
    let mut pairs = Vec::new();
    let mut taken_before: BTreeSet<usize> = BTreeSet::new();
    let mut taken_after: BTreeSet<usize> = BTreeSet::new();

    for (left_index, left) in before.functions.iter().enumerate() {
        let found = after.functions.iter().enumerate().find(|(right_index, right)| {
            !taken_after.contains(right_index)
                && right.shape.text_hash() == left.shape.text_hash()
                && right.params == left.params
        });

        if let Some((right_index, right)) = found {
            taken_before.insert(left_index);
            taken_after.insert(right_index);
            pairs.push(Pair {
                before: left.name.clone(),
                after: right.name.clone(),
                verdict: if left.name == right.name {
                    Verdict::Identical
                } else {
                    Verdict::Renamed
                },
                similarity: 1.0,
            });
        }
    }

    let mut scored: Vec<(f64, usize, usize)> = Vec::new();

    for (left_index, left) in before.functions.iter().enumerate() {
        if taken_before.contains(&left_index) {
            continue;
        }

        for (right_index, right) in after.functions.iter().enumerate() {
            if taken_after.contains(&right_index) {
                continue;
            }

            let similarity = left.shape.similarity(&right.shape, GRAM_WIDTH);
            if similarity >= threshold {
                scored.push((similarity, left_index, right_index));
            }
        }
    }

    scored.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.1.cmp(&right.1))
    });

    for (similarity, left_index, right_index) in scored {
        if taken_before.contains(&left_index) || taken_after.contains(&right_index) {
            continue;
        }

        taken_before.insert(left_index);
        taken_after.insert(right_index);

        pairs.push(Pair {
            before: before.functions[left_index].name.clone(),
            after: after.functions[right_index].name.clone(),
            verdict: Verdict::Edited,
            similarity,
        });
    }

    let gone = before
        .functions
        .iter()
        .enumerate()
        .filter(|(index, _)| !taken_before.contains(index))
        .map(|(_, function)| function.name.clone())
        .collect();

    let added = after
        .functions
        .iter()
        .enumerate()
        .filter(|(index, _)| !taken_after.contains(index))
        .map(|(_, function)| function.name.clone())
        .collect();

    pairs.sort_by(|left, right| left.before.cmp(&right.before));

    BuildDiff { pairs, gone, added }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub name: String,
    pub params: usize,
    pub text_hash: u64,
    pub skeleton_hash: u64,
    #[serde(default)]
    pub signature: Signature,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub evidence: Vec<String>,
}

impl Binding {
    pub fn of(function: &FunctionShape) -> Self {
        Self {
            name: function.name.clone(),
            params: function.params,
            text_hash: function.shape.text_hash(),
            skeleton_hash: function.shape.skeleton_hash(),
            signature: Signature::of(function),
            score: 0.0,
            evidence: Vec::new(),
        }
    }
}

pub const LOCK_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lock {
    #[serde(default)]
    pub version: u32,
    pub script_digest: String,
    #[serde(default)]
    pub note: String,
    pub roles: BTreeMap<String, Binding>,
}

impl Lock {
    pub fn from_resolution(
        script_digest: impl Into<String>,
        index: &ShapeIndex,
        resolution: &Resolution,
    ) -> Self {
        let mut roles = BTreeMap::new();

        for (role, candidate) in &resolution.roles {
            let Some(function) = index.get(&candidate.name) else {
                continue;
            };

            let mut binding = Binding::of(function);
            binding.score = candidate.score;
            binding.evidence = candidate.matched.clone();
            roles.insert(role.clone(), binding);
        }

        Self {
            version: LOCK_VERSION,
            script_digest: script_digest.into(),
            note: String::new(),
            roles,
        }
    }

    pub fn readable(&self) -> Result<()> {
        if self.version == LOCK_VERSION {
            return Ok(());
        }

        Err(Error::msg(format!(
            "this lock was written in format {}, this build reads format {LOCK_VERSION}, \
             re-run `wre locate --lock` against the build it was made from",
            self.version
        )))
    }

    pub fn check(&self, index: &ShapeIndex, threshold: f64) -> Vec<RoleDrift> {
        self.roles
            .iter()
            .map(|(role, binding)| RoleDrift {
                role: role.clone(),
                state: relocate(binding, index, threshold),
            })
            .collect()
    }

    pub fn is_current(&self, digest: &str) -> bool {
        self.script_digest == digest
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum State {
    Intact,
    Renamed { to: String },
    Edited { to: String, similarity: f64 },
    Lost,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleDrift {
    pub role: String,
    #[serde(flatten)]
    pub state: State,
}

impl RoleDrift {
    pub fn needs_review(&self) -> bool {
        !matches!(self.state, State::Intact)
    }

    pub fn describe(&self) -> String {
        match &self.state {
            State::Intact => format!("{}: unchanged", self.role),
            State::Renamed { to } => format!("{}: renamed to {to}, body unchanged", self.role),
            State::Edited { to, similarity } => format!(
                "{}: now {to}, body changed, {:.1}% of the structure is shared",
                self.role,
                similarity * 100.0
            ),
            State::Lost => format!("{}: no longer findable, re-run the rules", self.role),
        }
    }
}

fn relocate(binding: &Binding, index: &ShapeIndex, threshold: f64) -> State {
    let exact = index
        .functions
        .iter()
        .find(|function| function.shape.text_hash() == binding.text_hash);

    if let Some(function) = exact {
        return if function.name == binding.name {
            State::Intact
        } else {
            State::Renamed { to: function.name.clone() }
        };
    }

    let mut best: Option<(f64, &FunctionShape)> = None;

    for function in &index.functions {
        let similarity = grams_similarity(binding, function);
        if similarity >= threshold && best.as_ref().is_none_or(|(score, _)| similarity > *score) {
            best = Some((similarity, function));
        }
    }

    match best {
        Some((similarity, function)) => State::Edited {
            to: function.name.clone(),
            similarity,
        },
        None => State::Lost,
    }
}

fn grams_similarity(binding: &Binding, function: &FunctionShape) -> f64 {
    if binding.skeleton_hash == function.shape.skeleton_hash() {
        return 1.0;
    }
    binding.signature.estimate(&Signature::of(function))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wre_js::pipeline::SourceKind;

    const BUILD_ONE: &str = r#"
        function hash(s) {
            var h = 2166136261;
            for (var i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = h * 16777619; }
            return h >>> 0;
        }
        function toHex(bytes) {
            var out = "";
            for (var i = 0; i < bytes.length; i++) { out += bytes[i].toString(16); }
            return out;
        }
        function retired() { return 1; }
    "#;

    const BUILD_TWO: &str = r#"
        function Qa(s) {
            var h = 2166136261;
            for (var i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = h * 16777619; }
            return h >>> 0;
        }
        function toHex(bytes) {
            var out = "";
            for (var i = 0; i < bytes.length; i++) { out += bytes[i].toString(16).padStart(2, "0"); }
            return out;
        }
        function freshlyAdded(a, b, c) { return a + b + c; }
    "#;

    fn index(source: &str) -> ShapeIndex {
        ShapeIndex::build(source, SourceKind::Script).unwrap()
    }

    #[test]
    fn an_untouched_function_is_reported_as_identical() {
        let diff = compare(&index(BUILD_ONE), &index(BUILD_ONE), 0.5);

        assert!(diff.pairs.iter().all(|pair| pair.verdict == Verdict::Identical));
        assert!(diff.gone.is_empty());
        assert!(diff.added.is_empty());
    }

    #[test]
    fn a_renamed_function_is_followed_across_builds() {
        let diff = compare(&index(BUILD_ONE), &index(BUILD_TWO), 0.5);
        let pair = diff.follow("hash").expect("hash was not paired");

        assert_eq!(pair.after, "Qa");
        assert_eq!(pair.verdict, Verdict::Renamed);
        assert_eq!(diff.renamed().get("hash"), Some(&"Qa".to_string()));
    }

    #[test]
    fn an_edited_function_is_paired_and_flagged() {
        let diff = compare(&index(BUILD_ONE), &index(BUILD_TWO), 0.5);
        let pair = diff.follow("toHex").expect("toHex was not paired");

        assert_eq!(pair.verdict, Verdict::Edited);
        assert!(pair.similarity > 0.5 && pair.similarity < 1.0);
    }

    #[test]
    fn removals_and_additions_are_listed_separately() {
        let diff = compare(&index(BUILD_ONE), &index(BUILD_TWO), 0.5);

        assert_eq!(diff.gone, vec!["retired".to_string()]);
        assert_eq!(diff.added, vec!["freshlyAdded".to_string()]);
        assert!(diff.summary().contains("1 gone"));
    }

    #[test]
    fn no_function_is_paired_twice() {
        let diff = compare(&index(BUILD_ONE), &index(BUILD_TWO), 0.0);

        let befores: BTreeSet<&String> = diff.pairs.iter().map(|pair| &pair.before).collect();
        let afters: BTreeSet<&String> = diff.pairs.iter().map(|pair| &pair.after).collect();

        assert_eq!(befores.len(), diff.pairs.len());
        assert_eq!(afters.len(), diff.pairs.len());
    }

    fn lock_of(source: &str) -> Lock {
        let index = index(source);
        let mut roles = BTreeMap::new();
        roles.insert("hash".to_string(), Binding::of(index.get("hash").unwrap()));
        roles.insert("to-hex".to_string(), Binding::of(index.get("toHex").unwrap()));
        roles.insert("retired".to_string(), Binding::of(index.get("retired").unwrap()));

        Lock {
            version: LOCK_VERSION,
            script_digest: "abc123".to_string(),
            note: String::new(),
            roles,
        }
    }

    #[test]
    fn a_lock_from_an_older_format_is_reported_clearly() {
        let mut lock = lock_of(BUILD_ONE);
        lock.version = 0;

        let error = lock.readable().unwrap_err().to_string();
        assert!(error.contains("format 0"), "{error}");
        assert!(error.contains("wre locate --lock"), "{error}");

        lock.version = LOCK_VERSION;
        assert!(lock.readable().is_ok());
    }

    #[test]
    fn a_lock_against_its_own_build_reports_nothing_to_review() {
        let lock = lock_of(BUILD_ONE);
        let drift = lock.check(&index(BUILD_ONE), 0.5);

        assert!(drift.iter().all(|entry| !entry.needs_review()));
        assert!(lock.is_current("abc123"));
        assert!(!lock.is_current("other"));
    }

    #[test]
    fn a_lock_against_a_new_build_names_what_moved() {
        let drift = lock_of(BUILD_ONE).check(&index(BUILD_TWO), 0.5);
        let by_role: BTreeMap<String, State> = drift
            .iter()
            .map(|entry| (entry.role.clone(), entry.state.clone()))
            .collect();

        assert_eq!(by_role.get("hash"), Some(&State::Renamed { to: "Qa".to_string() }));
        assert_eq!(by_role.get("retired"), Some(&State::Lost));

        match by_role.get("to-hex") {
            Some(State::Edited { to, similarity }) => {
                assert_eq!(to, "toHex");
                assert!(*similarity > 0.5 && *similarity < 1.0, "similarity was {similarity}");
            }
            other => panic!("expected an edit, got {other:?}"),
        }
    }

    #[test]
    fn a_signature_estimates_the_true_overlap() {
        let one = index(BUILD_ONE);
        let two = index(BUILD_TWO);

        let before = one.get("toHex").unwrap();
        let after = two.get("toHex").unwrap();

        let exact = before.shape.similarity(&after.shape, GRAM_WIDTH);
        let estimated = Signature::of(before).estimate(&Signature::of(after));

        assert!((exact - estimated).abs() < 0.2, "exact {exact}, estimated {estimated}");
    }

    #[test]
    fn a_signature_of_the_same_function_estimates_one() {
        let one = index(BUILD_ONE);
        let shape = one.get("hash").unwrap();
        assert_eq!(Signature::of(shape).estimate(&Signature::of(shape)), 1.0);
    }

    #[test]
    fn a_signature_stays_small_for_a_large_function() {
        let mut source = String::from("function big() {");
        for index in 0..500 {
            source.push_str(&format!("var v{index} = {index} + 1;"));
        }
        source.push('}');

        let built = index(&source);
        let signature = Signature::of(built.get("big").unwrap());
        assert!(signature.values.len() <= SIGNATURE_WIDTH);
    }

    #[test]
    fn every_drift_entry_explains_itself() {
        for entry in lock_of(BUILD_ONE).check(&index(BUILD_TWO), 0.5) {
            assert!(entry.describe().starts_with(&entry.role));
        }
    }

    #[test]
    fn a_lock_is_built_from_a_resolution_and_round_trips() {
        use crate::locate::{Clue, Evidence, Locator, Rule};

        let index = index(BUILD_ONE);
        let rule = Rule::new(
            "hash",
            vec![Clue::new(Evidence::Constants { any: vec![2166136261.0] }, 1.0).required()],
        );

        let resolution = Locator::new(&index).resolve(&[rule]).unwrap();
        let lock = Lock::from_resolution("digest", &index, &resolution);

        assert_eq!(lock.roles.get("hash").unwrap().name, "hash");
        assert!(!lock.roles.get("hash").unwrap().evidence.is_empty());

        let text = serde_json::to_string(&lock).unwrap();
        assert_eq!(serde_json::from_str::<Lock>(&text).unwrap(), lock);
    }
}
