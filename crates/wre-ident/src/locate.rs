use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

use crate::shape::{FunctionShape, ShapeIndex};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Evidence {
    ShapeText { pattern: String },
    SkeletonHash { hash: u64 },
    Arity { params: usize },
    Constants { any: Vec<f64> },
    Strings { any: Vec<String> },
    Properties { all: Vec<String> },
    ObjectKeys { all: Vec<String> },
    Calls { role: String },
    CalledBy { role: String },
    Size { least: usize, most: usize },
    Loops { least: usize },
    Behaves { vector: TestVector },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestVector {
    pub arguments: Vec<serde_json::Value>,
    pub expect: serde_json::Value,
    #[serde(default)]
    pub note: String,
}

impl TestVector {
    pub fn new(arguments: Vec<serde_json::Value>, expect: serde_json::Value) -> Self {
        Self { arguments, expect, note: String::new() }
    }
}

pub trait Oracle {
    fn call(&self, function: &str, vector: &TestVector) -> Option<serde_json::Value>;
}

pub struct NoOracle;

impl Oracle for NoOracle {
    fn call(&self, _function: &str, _vector: &TestVector) -> Option<serde_json::Value> {
        None
    }
}

impl Evidence {
    pub fn describe(&self) -> String {
        match self {
            Evidence::ShapeText { pattern } => format!("normalised text matches /{pattern}/"),
            Evidence::SkeletonHash { hash } => format!("structure hashes to {hash:016x}"),
            Evidence::Arity { params } => format!("takes {params} parameters"),
            Evidence::Constants { any } => format!("uses one of the constants {any:?}"),
            Evidence::Strings { any } => format!("carries one of the strings {any:?}"),
            Evidence::Properties { all } => format!("reaches every property of {all:?}"),
            Evidence::ObjectKeys { all } => format!("builds an object carrying {all:?}"),
            Evidence::Calls { role } => format!("calls whatever fills the {role} role"),
            Evidence::CalledBy { role } => format!("is called by the {role} role"),
            Evidence::Size { least, most } => format!("holds {least} to {most} statements"),
            Evidence::Loops { least } => format!("holds at least {least} loops"),
            Evidence::Behaves { vector } => {
                if vector.note.is_empty() {
                    format!("called with {:?} it returns {}", vector.arguments, vector.expect)
                } else {
                    vector.note.clone()
                }
            }
        }
    }

    pub fn needs_roles(&self) -> Option<&str> {
        match self {
            Evidence::Calls { role } | Evidence::CalledBy { role } => Some(role),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clue {
    #[serde(flatten)]
    pub evidence: Evidence,
    #[serde(default = "one")]
    pub weight: f64,
    #[serde(default)]
    pub required: bool,
}

fn one() -> f64 {
    1.0
}

impl Clue {
    pub fn new(evidence: Evidence, weight: f64) -> Self {
        Self { evidence, weight, required: false }
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub role: String,
    pub clues: Vec<Clue>,
    #[serde(default = "half")]
    pub minimum: f64,
    #[serde(default = "margin")]
    pub margin: f64,
}

fn half() -> f64 {
    0.5
}

fn margin() -> f64 {
    0.05
}

impl Rule {
    pub fn new(role: impl Into<String>, clues: Vec<Clue>) -> Self {
        Self {
            role: role.into(),
            clues,
            minimum: half(),
            margin: margin(),
        }
    }

    pub fn total_weight(&self) -> f64 {
        self.clues.iter().map(|clue| clue.weight).sum()
    }

    pub fn depends_on(&self) -> BTreeSet<String> {
        self.clues
            .iter()
            .filter_map(|clue| clue.evidence.needs_roles().map(str::to_string))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub name: String,
    pub score: f64,
    pub matched: Vec<String>,
    pub missed: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Resolution {
    pub roles: BTreeMap<String, Candidate>,
    pub runners_up: BTreeMap<String, Vec<Candidate>>,
    pub ambiguous: Vec<String>,
    pub unresolved: Vec<String>,
}

impl Resolution {
    pub fn binding(&self, role: &str) -> Option<&str> {
        self.roles.get(role).map(|candidate| candidate.name.as_str())
    }

    pub fn is_complete(&self) -> bool {
        self.unresolved.is_empty() && self.ambiguous.is_empty()
    }
}

pub struct Locator<'i> {
    index: &'i ShapeIndex,
    callers: BTreeMap<String, BTreeSet<String>>,
    oracle: Option<&'i dyn Oracle>,
}

impl<'i> Locator<'i> {
    pub fn with_oracle(mut self, oracle: &'i dyn Oracle) -> Self {
        self.oracle = Some(oracle);
        self
    }

    pub fn new(index: &'i ShapeIndex) -> Self {
        let mut callers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

        for function in &index.functions {
            for callee in &function.facts.calls {
                callers
                    .entry(callee.clone())
                    .or_default()
                    .insert(function.name.clone());
            }
        }

        Self { index, callers, oracle: None }
    }

    pub fn resolve(&self, rules: &[Rule]) -> Result<Resolution> {
        for rule in rules {
            for clue in &rule.clues {
                if let Evidence::ShapeText { pattern } = &clue.evidence {
                    regex::Regex::new(pattern).map_err(|error| {
                        Error::msg(format!("bad pattern for role {}: {error}", rule.role))
                    })?;
                }
            }
        }

        let mut resolution = Resolution::default();
        let mut claimed: BTreeSet<String> = BTreeSet::new();
        let mut pending: Vec<&Rule> = rules.iter().collect();

        loop {
            let ready: Vec<&Rule> = pending
                .iter()
                .copied()
                .filter(|rule| {
                    rule.depends_on()
                        .iter()
                        .all(|role| resolution.roles.contains_key(role))
                })
                .collect();

            if ready.is_empty() {
                break;
            }

            let mut settled_any = false;

            for rule in ready {
                let mut ranked = self.rank(rule, &resolution, &claimed);
                ranked.sort_by(|left, right| {
                    right
                        .score
                        .partial_cmp(&left.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(left.name.cmp(&right.name))
                });

                let winner = ranked.first().cloned();
                let runner = ranked.get(1).cloned();

                match winner {
                    Some(best) if best.score >= rule.minimum => {
                        let too_close = runner
                            .as_ref()
                            .is_some_and(|next| best.score - next.score < rule.margin);

                        if too_close {
                            resolution.ambiguous.push(rule.role.clone());
                        } else {
                            claimed.insert(best.name.clone());
                            resolution.roles.insert(rule.role.clone(), best);
                            settled_any = true;
                        }
                    }
                    _ => {
                        resolution.unresolved.push(rule.role.clone());
                    }
                }

                resolution
                    .runners_up
                    .insert(rule.role.clone(), ranked.into_iter().take(4).collect());

                pending.retain(|entry| entry.role != rule.role);
            }

            if !settled_any {
                break;
            }
        }

        for rule in pending {
            if !resolution.unresolved.contains(&rule.role) {
                resolution.unresolved.push(rule.role.clone());
            }
        }

        resolution.unresolved.sort();
        resolution.unresolved.dedup();
        resolution.ambiguous.sort();
        resolution.ambiguous.dedup();

        Ok(resolution)
    }

    fn rank(
        &self,
        rule: &Rule,
        resolution: &Resolution,
        claimed: &BTreeSet<String>,
    ) -> Vec<Candidate> {
        let total = rule.total_weight();
        if total <= 0.0 {
            return Vec::new();
        }

        self.index
            .functions
            .iter()
            .filter(|function| !claimed.contains(&function.name))
            .filter_map(|function| {
                let mut earned = 0.0;
                let mut matched = Vec::new();
                let mut missed = Vec::new();

                for clue in &rule.clues {
                    if self.holds(&clue.evidence, function, resolution) {
                        earned += clue.weight;
                        matched.push(clue.evidence.describe());
                    } else {
                        if clue.required {
                            return None;
                        }
                        missed.push(clue.evidence.describe());
                    }
                }

                Some(Candidate {
                    name: function.name.clone(),
                    score: earned / total,
                    matched,
                    missed,
                })
            })
            .collect()
    }

    fn holds(
        &self,
        evidence: &Evidence,
        function: &FunctionShape,
        resolution: &Resolution,
    ) -> bool {
        match evidence {
            Evidence::ShapeText { pattern } => regex::Regex::new(pattern)
                .map(|regex| regex.is_match(&function.text()))
                .unwrap_or(false),

            Evidence::SkeletonHash { hash } => function.shape.skeleton_hash() == *hash,

            Evidence::Arity { params } => function.params == *params,

            Evidence::Constants { any } => any.iter().any(|value| function.has_number(*value)),

            Evidence::Strings { any } => any
                .iter()
                .any(|wanted| function.facts.strings.iter().any(|found| found.contains(wanted))),

            Evidence::Properties { all } => {
                all.iter().all(|name| function.facts.properties.contains(name))
            }

            Evidence::ObjectKeys { all } => function.has_object_with(all),

            Evidence::Calls { role } => resolution
                .binding(role)
                .is_some_and(|target| function.facts.calls.contains(target)),

            Evidence::CalledBy { role } => resolution.binding(role).is_some_and(|target| {
                self.callers
                    .get(&function.name)
                    .is_some_and(|callers| callers.contains(target))
            }),

            Evidence::Size { least, most } => {
                function.facts.statements >= *least && function.facts.statements <= *most
            }

            Evidence::Loops { least } => function.facts.loops >= *least,

            Evidence::Behaves { vector } => self
                .oracle
                .and_then(|oracle| oracle.call(&function.name, vector))
                .is_some_and(|got| got == vector.expect),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wre_js::pipeline::SourceKind;

    const BUNDLE: &str = r#"
        function a1(s) {
            var h = 2166136261;
            for (var i = 0; i < s.length; i++) {
                h ^= s.charCodeAt(i);
                h = h * 16777619;
            }
            return h >>> 0;
        }

        function b2(bytes) {
            var out = "";
            for (var i = 0; i < bytes.length; i++) {
                out += bytes[i].toString(16).padStart(2, "0");
            }
            return out;
        }

        function c3(text) {
            return a1(text) + 1;
        }

        function d4() {
            return { key: "collector", sources: [1, 2], stage: 3 };
        }
    "#;

    const REBUILT: &str = r#"
        function ZZ(q) {
            var w = 2166136261;
            for (var e = 0; e < q.length; e++) {
                w ^= q.charCodeAt(e);
                w = w * 16777619;
            }
            return w >>> 0;
        }

        function YY(v) {
            var r = "";
            for (var t = 0; t < v.length; t++) {
                r += v[t].toString(16).padStart(2, "0");
            }
            return r;
        }

        function XX(u) {
            return ZZ(u) + 1;
        }

        function WW() {
            return { key: "collector", sources: [4, 5], stage: 9 };
        }
    "#;

    fn rules() -> Vec<Rule> {
        vec![
            Rule::new(
                "hash",
                vec![
                    Clue::new(Evidence::Constants { any: vec![2166136261.0] }, 3.0).required(),
                    Clue::new(Evidence::Arity { params: 1 }, 1.0),
                    Clue::new(Evidence::Loops { least: 1 }, 1.0),
                ],
            ),
            Rule::new(
                "to-hex",
                vec![
                    Clue::new(
                        Evidence::Properties {
                            all: vec!["toString".to_string(), "padStart".to_string()],
                        },
                        3.0,
                    ),
                    Clue::new(Evidence::Arity { params: 1 }, 1.0),
                ],
            ),
            Rule::new(
                "registry",
                vec![Clue::new(
                    Evidence::ObjectKeys {
                        all: vec!["key".to_string(), "sources".to_string()],
                    },
                    4.0,
                )
                .required()],
            ),
            Rule::new(
                "hash-caller",
                vec![
                    Clue::new(Evidence::Calls { role: "hash".to_string() }, 4.0).required(),
                    Clue::new(Evidence::Arity { params: 1 }, 1.0),
                ],
            ),
        ]
    }

    fn resolve(source: &str) -> Resolution {
        let index = ShapeIndex::build(source, SourceKind::Script).unwrap();
        Locator::new(&index).resolve(&rules()).unwrap()
    }

    #[test]
    fn every_role_binds_in_the_first_build() {
        let found = resolve(BUNDLE);

        assert_eq!(found.binding("hash"), Some("a1"));
        assert_eq!(found.binding("to-hex"), Some("b2"));
        assert_eq!(found.binding("registry"), Some("d4"));
        assert_eq!(found.binding("hash-caller"), Some("c3"));
        assert!(found.is_complete(), "{:?}", found.unresolved);
    }

    #[test]
    fn a_full_rename_does_not_break_a_single_role() {
        let found = resolve(REBUILT);

        assert_eq!(found.binding("hash"), Some("ZZ"));
        assert_eq!(found.binding("to-hex"), Some("YY"));
        assert_eq!(found.binding("registry"), Some("WW"));
        assert_eq!(found.binding("hash-caller"), Some("XX"));
        assert!(found.is_complete());
    }

    #[test]
    fn call_graph_adjacency_resolves_after_the_role_it_depends_on() {
        let index = ShapeIndex::build(REBUILT, SourceKind::Script).unwrap();
        let locator = Locator::new(&index);

        let mut ordered = rules();
        ordered.reverse();

        let found = locator.resolve(&ordered).unwrap();
        assert_eq!(found.binding("hash-caller"), Some("XX"));
    }

    #[test]
    fn a_required_clue_that_fails_removes_the_candidate_outright() {
        let index = ShapeIndex::build(BUNDLE, SourceKind::Script).unwrap();
        let locator = Locator::new(&index);

        let rule = Rule::new(
            "impossible",
            vec![Clue::new(Evidence::Constants { any: vec![424242.0] }, 1.0).required()],
        );

        let found = locator.resolve(&[rule]).unwrap();
        assert_eq!(found.unresolved, vec!["impossible".to_string()]);
        assert!(found.roles.is_empty());
    }

    #[test]
    fn two_equally_good_candidates_are_reported_as_ambiguous() {
        let source = "function one(a) { return a; } function two(b) { return b; }";
        let index = ShapeIndex::build(source, SourceKind::Script).unwrap();
        let locator = Locator::new(&index);

        let rule = Rule::new("either", vec![Clue::new(Evidence::Arity { params: 1 }, 1.0)]);
        let found = locator.resolve(&[rule]).unwrap();

        assert_eq!(found.ambiguous, vec!["either".to_string()]);
        assert!(found.binding("either").is_none());
        assert!(!found.is_complete());
    }

    #[test]
    fn the_evidence_behind_a_binding_is_reported() {
        let found = resolve(BUNDLE);
        let hash = found.roles.get("hash").unwrap();

        assert!(hash.matched.iter().any(|note| note.contains("2166136261")));
        assert!(hash.missed.is_empty());
        assert!((hash.score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn runners_up_are_kept_for_inspection() {
        let found = resolve(BUNDLE);
        let ranked = found.runners_up.get("to-hex").unwrap();

        assert!(ranked.len() > 1);
        assert_eq!(ranked[0].name, "b2");
        assert!(ranked[0].score > ranked[1].score);
    }

    #[test]
    fn a_bad_pattern_is_rejected_before_any_work() {
        let index = ShapeIndex::build(BUNDLE, SourceKind::Script).unwrap();
        let rule = Rule::new(
            "broken",
            vec![Clue::new(Evidence::ShapeText { pattern: "([unclosed".to_string() }, 1.0)],
        );

        let error = Locator::new(&index).resolve(&[rule]).unwrap_err().to_string();
        assert!(error.contains("bad pattern for role broken"), "{error}");
    }

    #[test]
    fn a_pattern_over_the_normalised_text_still_works_after_renaming() {
        let index = ShapeIndex::build(REBUILT, SourceKind::Script).unwrap();
        let rule = Rule::new(
            "hash",
            vec![Clue::new(
                Evidence::ShapeText { pattern: r"\.charCodeAt.*16777619".to_string() },
                1.0,
            )],
        );

        let found = Locator::new(&index).resolve(&[rule]).unwrap();
        assert_eq!(found.binding("hash"), Some("ZZ"));
    }

    struct FnvOracle;

    impl Oracle for FnvOracle {
        fn call(&self, function: &str, vector: &TestVector) -> Option<serde_json::Value> {
            let input = vector.arguments.first()?.as_str()?;

            let mut hash: u32 = 2166136261;
            for byte in input.bytes() {
                hash ^= u32::from(byte);
                hash = hash.wrapping_mul(16777619);
            }

            match function {
                "a1" | "ZZ" => Some(serde_json::json!(hash)),
                "c3" | "XX" => Some(serde_json::json!(hash.wrapping_add(1))),
                _ => None,
            }
        }
    }

    #[test]
    fn behaviour_alone_can_pin_a_role() {
        let index = ShapeIndex::build(REBUILT, SourceKind::Script).unwrap();
        let oracle = FnvOracle;

        let rule = Rule::new(
            "hash",
            vec![Clue::new(
                Evidence::Behaves {
                    vector: TestVector::new(
                        vec![serde_json::json!("abc")],
                        serde_json::json!(440920331u32),
                    ),
                },
                1.0,
            )
            .required()],
        );

        let found = Locator::new(&index).with_oracle(&oracle).resolve(&[rule]).unwrap();
        assert_eq!(found.binding("hash"), Some("ZZ"));
    }

    #[test]
    fn without_an_oracle_a_behaviour_clue_simply_does_not_hold() {
        let index = ShapeIndex::build(REBUILT, SourceKind::Script).unwrap();

        let rule = Rule::new(
            "hash",
            vec![Clue::new(
                Evidence::Behaves {
                    vector: TestVector::new(vec![], serde_json::json!(1)),
                },
                1.0,
            )
            .required()],
        );

        let found = Locator::new(&index).resolve(&[rule]).unwrap();
        assert_eq!(found.unresolved, vec!["hash".to_string()]);
    }

    #[test]
    fn a_wrong_expectation_rejects_the_right_function() {
        let index = ShapeIndex::build(REBUILT, SourceKind::Script).unwrap();

        let rule = Rule::new(
            "hash",
            vec![Clue::new(
                Evidence::Behaves {
                    vector: TestVector::new(
                        vec![serde_json::json!("abc")],
                        serde_json::json!(0),
                    ),
                },
                1.0,
            )
            .required()],
        );

        let found = Locator::new(&index).with_oracle(&FnvOracle).resolve(&[rule]).unwrap();
        assert!(found.binding("hash").is_none());
    }

    #[test]
    fn a_rule_round_trips_through_json() {
        for rule in rules() {
            let text = serde_json::to_string(&rule).unwrap();
            assert_eq!(serde_json::from_str::<Rule>(&text).unwrap(), rule);
        }
    }
}
