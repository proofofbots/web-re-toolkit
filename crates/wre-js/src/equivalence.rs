use std::collections::{BTreeMap, BTreeSet};

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::{ParseOptions, Parser};
use oxc_semantic::{Scoping, SemanticBuilder};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use wre_core::error::{Error, Result};

use crate::pipeline::SourceKind;

pub fn free_identifiers(source: &str, kind: SourceKind) -> Result<BTreeSet<String>> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, kind.to_source_type())
        .with_options(ParseOptions {
            preserve_parens: false,
            ..ParseOptions::default()
        })
        .parse();

    if parsed.panicked {
        return Err(Error::msg("the source does not parse"));
    }

    let program = parsed.program;
    let scoping: Scoping = SemanticBuilder::new().build(&program).semantic.into_scoping();

    let mut collector = FreeCollector { scoping: &scoping, names: BTreeSet::new() };
    collector.visit_program(&program);

    Ok(collector.names)
}

struct FreeCollector<'s> {
    scoping: &'s Scoping,
    names: BTreeSet<String>,
}

impl<'a, 's> Visit<'a> for FreeCollector<'s> {
    fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
        let resolved = identifier
            .reference_id
            .get()
            .and_then(|reference_id| self.scoping.get_reference(reference_id).symbol_id())
            .is_some();

        if !resolved {
            self.names.insert(identifier.name.as_str().to_string());
        }

        walk::walk_identifier_reference(self, identifier);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Equivalence {
    pub reached_for: Vec<String>,
    pub no_longer_reached_for: Vec<String>,
    pub disagreeing_values: Vec<String>,
}

impl Equivalence {
    pub fn holds(&self) -> bool {
        self.reached_for.is_empty() && self.disagreeing_values.is_empty()
    }

    pub fn describe(&self) -> String {
        if self.holds() {
            return "the rewrite reaches for nothing new and every checked value agrees".to_string();
        }

        let mut parts = Vec::new();

        if !self.reached_for.is_empty() {
            parts.push(format!(
                "the rewrite reaches for {} that the original never did",
                self.reached_for.join(", ")
            ));
        }

        if !self.disagreeing_values.is_empty() {
            parts.push(format!(
                "{} no longer produce the same value",
                self.disagreeing_values.join(", ")
            ));
        }

        parts.join("; ")
    }
}

pub fn compare(original: &str, rewritten: &str, kind: SourceKind) -> Result<Equivalence> {
    let before = free_identifiers(original, kind)?;
    let after = free_identifiers(rewritten, kind)?;

    Ok(Equivalence {
        reached_for: after.difference(&before).cloned().collect(),
        no_longer_reached_for: before.difference(&after).cloned().collect(),
        disagreeing_values: Vec::new(),
    })
}

pub fn compare_values(
    equivalence: &mut Equivalence,
    before: &BTreeMap<String, Value>,
    after: &BTreeMap<String, Value>,
) {
    for (label, value) in before {
        match after.get(label) {
            Some(found) if found == value => {}
            _ => equivalence.disagreeing_values.push(label.clone()),
        }
    }

    for label in after.keys() {
        if !before.contains_key(label) {
            equivalence.disagreeing_values.push(label.clone());
        }
    }

    equivalence.disagreeing_values.sort();
    equivalence.disagreeing_values.dedup();
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGINAL: &str = r#"
        var alias = atob;
        function decode(text) {
            var out = alias(text);
            return out.length;
        }
    "#;

    #[test]
    fn a_faithful_rewrite_holds() {
        let rewritten = r#"
            function decode(text) {
                var out = atob(text);
                return out.length;
            }
        "#;

        let found = compare(ORIGINAL, rewritten, SourceKind::Script).unwrap();
        assert!(found.holds(), "{}", found.describe());
        assert!(found.reached_for.is_empty());
    }

    #[test]
    fn a_pass_that_orphans_a_binding_is_caught() {
        let broken = r#"
            function decode(text) {
                var out = alias(text);
                return out.length;
            }
        "#;

        let found = compare(ORIGINAL, broken, SourceKind::Script).unwrap();
        assert!(!found.holds());
        assert_eq!(found.reached_for, vec!["alias".to_string()]);
        assert!(found.describe().contains("reaches for alias"));
    }

    #[test]
    fn dropping_a_global_read_is_noted_but_not_a_failure() {
        let trimmed = "function decode(text) { return text.length; }";

        let found = compare(ORIGINAL, trimmed, SourceKind::Script).unwrap();
        assert!(found.holds());
        assert_eq!(found.no_longer_reached_for, vec!["atob".to_string()]);
    }

    #[test]
    fn free_identifiers_ignore_anything_declared_in_scope() {
        let names = free_identifiers(
            "function run() { var local = 1; return local + globalThing; }",
            SourceKind::Script,
        )
        .unwrap();

        assert!(names.contains("globalThing"));
        assert!(!names.contains("local"));
        assert!(!names.contains("run"));
    }

    #[test]
    fn source_that_does_not_parse_is_reported() {
        assert!(free_identifiers("function (", SourceKind::Script).is_err());
        assert!(compare("var a = 1;", "function (", SourceKind::Script).is_err());
    }

    #[test]
    fn a_decoded_table_that_changed_is_caught() {
        let mut found = Equivalence::default();

        let before = BTreeMap::from([
            ("table.0".to_string(), Value::from("navigator")),
            ("table.1".to_string(), Value::from("userAgent")),
        ]);
        let after = BTreeMap::from([
            ("table.0".to_string(), Value::from("navigator")),
            ("table.1".to_string(), Value::from("userAgentData")),
        ]);

        compare_values(&mut found, &before, &after);

        assert!(!found.holds());
        assert_eq!(found.disagreeing_values, vec!["table.1".to_string()]);
    }

    #[test]
    fn a_table_that_survives_intact_leaves_the_verdict_alone() {
        let mut found = Equivalence::default();
        let table = BTreeMap::from([("table.0".to_string(), Value::from("navigator"))]);

        compare_values(&mut found, &table, &table);
        assert!(found.holds());
    }

    #[test]
    fn an_entry_that_appears_from_nowhere_is_caught() {
        let mut found = Equivalence::default();
        let before = BTreeMap::new();
        let after = BTreeMap::from([("table.9".to_string(), Value::from("extra"))]);

        compare_values(&mut found, &before, &after);
        assert_eq!(found.disagreeing_values, vec!["table.9".to_string()]);
    }
}
