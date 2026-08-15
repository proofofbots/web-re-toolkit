use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use wre_core::address::{Address, leaves};
use wre_core::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payload {
    pub value: Value,
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub codec: Option<String>,
}

impl Payload {
    pub fn new(value: Value) -> Self {
        Self { value, origin: None, codec: None }
    }

    pub fn from_origin(value: Value, origin: impl Into<String>, codec: impl Into<String>) -> Self {
        Self {
            value,
            origin: Some(origin.into()),
            codec: Some(codec.into()),
        }
    }

    pub fn get(&self, address: &Address) -> Option<&Value> {
        address.get(&self.value)
    }

    pub fn set(&mut self, address: &Address, value: Value) -> Result<()> {
        address.set(&mut self.value, value)
    }

    pub fn remove(&mut self, address: &Address) -> Option<Value> {
        address.remove(&mut self.value)
    }

    pub fn addresses(&self) -> Vec<Address> {
        leaves(&self.value)
            .into_iter()
            .map(|(address, _)| address)
            .collect()
    }

    pub fn leaf_count(&self) -> usize {
        leaves(&self.value).len()
    }

    pub fn find(&self, pattern: &Address) -> Vec<(Address, &Value)> {
        leaves(&self.value)
            .into_iter()
            .filter(|(address, _)| pattern.matches(address))
            .collect()
    }

    pub fn apply(&mut self, patches: &[Patch]) -> Result<usize> {
        let mut applied = 0usize;

        for patch in patches {
            match patch {
                Patch::Set { address, value } => {
                    address.set(&mut self.value, value.clone())?;
                    applied += 1;
                }
                Patch::Remove { address } => {
                    if address.remove(&mut self.value).is_some() {
                        applied += 1;
                    }
                }
                Patch::Replace { pattern, value } => {
                    let targets: Vec<Address> = leaves(&self.value)
                        .into_iter()
                        .filter(|(address, _)| pattern.matches(address))
                        .map(|(address, _)| address)
                        .collect();

                    for address in targets {
                        address.set(&mut self.value, value.clone())?;
                        applied += 1;
                    }
                }
                Patch::Substitute { from, to } => {
                    applied += substitute(&mut self.value, from, to);
                }
            }
        }

        Ok(applied)
    }
}

pub fn substitute(value: &mut Value, from: &str, to: &str) -> usize {
    match value {
        Value::String(text) => {
            if text.contains(from) {
                *text = text.replace(from, to);
                1
            } else {
                0
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .map(|item| substitute(item, from, to))
            .sum(),
        Value::Object(map) => map
            .iter_mut()
            .map(|(_, item)| substitute(item, from, to))
            .sum(),
        _ => 0,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "patch", rename_all = "lowercase")]
pub enum Patch {
    Set { address: Address, value: Value },
    Remove { address: Address },
    Replace { pattern: Address, value: Value },
    Substitute { from: String, to: String },
}

impl Patch {
    pub fn parse(spec: &str) -> Result<Self> {
        if let Some(rest) = spec.strip_prefix('-') {
            return Ok(Patch::Remove { address: Address::parse(rest)? });
        }

        if let Some(rest) = spec.strip_prefix("s/") {
            let (from, to) = split_unescaped(rest)
                .ok_or_else(|| Error::msg(format!("substitution {spec} needs s/from/to/")))?;
            return Ok(Patch::Substitute { from, to });
        }

        let (path, raw) = spec
            .split_once('=')
            .ok_or_else(|| Error::msg(format!("patch {spec} is not address=value")))?;

        let value = serde_json::from_str(raw)
            .unwrap_or_else(|_| Value::String(raw.to_string()));

        let address = Address::parse(path)?;

        if address.has_wildcard() {
            Ok(Patch::Replace { pattern: address, value })
        } else {
            Ok(Patch::Set { address, value })
        }
    }
}

fn split_unescaped(text: &str) -> Option<(String, String)> {
    let mut from = String::new();
    let mut chars = text.chars();
    let mut escaped = false;
    let mut closed = false;

    for ch in chars.by_ref() {
        if escaped {
            from.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '/' => {
                closed = true;
                break;
            }
            other => from.push(other),
        }
    }

    if !closed {
        return None;
    }

    let mut to = String::new();
    let mut escaped = false;

    for ch in chars {
        if escaped {
            to.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '/' => break,
            other => to.push(other),
        }
    }

    Some((from, to))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDiff {
    pub address: Address,
    pub change: Change,
    #[serde(default)]
    pub left: Option<Value>,
    #[serde(default)]
    pub right: Option<Value>,
}

impl FieldDiff {
    pub fn render(&self) -> String {
        match self.change {
            Change::Added => format!("+ {} = {}", self.address, render(&self.right)),
            Change::Removed => format!("- {} was {}", self.address, render(&self.left)),
            Change::Changed => format!(
                "~ {} {} -> {}",
                self.address,
                render(&self.left),
                render(&self.right)
            ),
        }
    }
}

fn render(value: &Option<Value>) -> String {
    match value {
        None => "absent".to_string(),
        Some(Value::String(text)) if text.len() > 48 => {
            format!("\"{}…\"", &text[..48.min(text.len())])
        }
        Some(other) => other.to_string(),
    }
}

pub fn diff(left: &Value, right: &Value) -> Vec<FieldDiff> {
    let left_leaves: BTreeMap<Address, Value> = leaves(left)
        .into_iter()
        .map(|(address, value)| (address, value.clone()))
        .collect();

    let right_leaves: BTreeMap<Address, Value> = leaves(right)
        .into_iter()
        .map(|(address, value)| (address, value.clone()))
        .collect();

    let mut out = Vec::new();
    let mut seen: BTreeSet<&Address> = BTreeSet::new();

    for (address, value) in &left_leaves {
        seen.insert(address);
        match right_leaves.get(address) {
            None => out.push(FieldDiff {
                address: address.clone(),
                change: Change::Removed,
                left: Some(value.clone()),
                right: None,
            }),
            Some(other) if other != value => out.push(FieldDiff {
                address: address.clone(),
                change: Change::Changed,
                left: Some(value.clone()),
                right: Some(other.clone()),
            }),
            Some(_) => {}
        }
    }

    for (address, value) in &right_leaves {
        if !seen.contains(address) {
            out.push(FieldDiff {
                address: address.clone(),
                change: Change::Added,
                left: None,
                right: Some(value.clone()),
            });
        }
    }

    out.sort_by(|a, b| a.address.cmp(&b.address));
    out
}

pub fn moved_addresses(diffs: &[FieldDiff]) -> BTreeSet<Address> {
    diffs.iter().map(|entry| entry.address.clone()).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeReport {
    pub applied: usize,
    pub from_donor: usize,
    pub overwritten: usize,
    pub addresses: usize,
}

pub fn forge(donor: &Payload, patches: &[Patch]) -> Result<(Payload, ForgeReport)> {
    let mut forged = donor.clone();
    let before = forged.leaf_count();
    let applied = forged.apply(patches)?;
    let changes = diff(&donor.value, &forged.value);

    Ok((
        forged,
        ForgeReport {
            applied,
            from_donor: before.saturating_sub(changes.len()),
            overwritten: changes
                .iter()
                .filter(|entry| entry.change == Change::Changed)
                .count(),
            addresses: before,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn diffs_by_address() {
        let left = json!({ "a": 1, "b": { "c": 2 }, "d": [1, 2] });
        let right = json!({ "a": 1, "b": { "c": 3 }, "d": [1, 2, 4] });

        let changes = diff(&left, &right);
        let rendered: Vec<String> = changes.iter().map(FieldDiff::render).collect();

        assert!(rendered.iter().any(|line| line.starts_with("~ b.c")), "{rendered:?}");
        assert!(rendered.iter().any(|line| line.starts_with("+ d[2]")), "{rendered:?}");
        assert_eq!(changes.len(), 2);
    }

    #[test]
    fn parses_and_applies_patches() {
        let mut payload = Payload::new(json!({ "s7": { "v": 1 }, "c": "key" }));

        let patches = vec![
            Patch::parse("s7.v=32").unwrap(),
            Patch::parse("c=\"other\"").unwrap(),
        ];

        assert_eq!(payload.apply(&patches).unwrap(), 2);
        assert_eq!(payload.get(&Address::parse("s7.v").unwrap()), Some(&json!(32)));
        assert_eq!(payload.get(&Address::parse("c").unwrap()), Some(&json!("other")));
    }

    #[test]
    fn removes_and_substitutes() {
        let mut payload = Payload::new(json!({
            "keep": 1,
            "drop": 2,
            "origin": "https://localhost:3000",
            "nested": { "url": "https://localhost:3000/x" }
        }));

        payload
            .apply(&[
                Patch::parse("-drop").unwrap(),
                Patch::parse("s/https:\\/\\/localhost:3000/https:\\/\\/demo.example.com/").unwrap(),
            ])
            .unwrap();

        assert!(payload.get(&Address::parse("drop").unwrap()).is_none());
        assert_eq!(
            payload.get(&Address::parse("origin").unwrap()),
            Some(&json!("https://demo.example.com"))
        );
        assert_eq!(
            payload.get(&Address::parse("nested.url").unwrap()),
            Some(&json!("https://demo.example.com/x"))
        );
    }

    #[test]
    fn wildcard_patches_hit_every_match() {
        let mut payload = Payload::new(json!({ "a": { "v": 1 }, "b": { "v": 2 } }));
        let applied = payload.apply(&[Patch::parse("*.v=9").unwrap()]).unwrap();
        assert_eq!(applied, 2);
        assert_eq!(payload.get(&Address::parse("a.v").unwrap()), Some(&json!(9)));
        assert_eq!(payload.get(&Address::parse("b.v").unwrap()), Some(&json!(9)));
    }

    #[test]
    fn forging_reports_what_moved() {
        let donor = Payload::new(json!({ "a": 1, "b": 2, "c": 3 }));
        let (forged, report) = forge(&donor, &[Patch::parse("b=20").unwrap()]).unwrap();

        assert_eq!(report.applied, 1);
        assert_eq!(report.overwritten, 1);
        assert_eq!(forged.get(&Address::parse("b").unwrap()), Some(&json!(20)));
        assert_eq!(forged.get(&Address::parse("a").unwrap()), Some(&json!(1)));
    }

    #[test]
    fn finds_addresses_by_pattern() {
        let payload = Payload::new(json!({ "s1": { "v": 1 }, "s2": { "v": 2 }, "meta": 0 }));
        let found = payload.find(&Address::parse("*.v").unwrap());
        assert_eq!(found.len(), 2);
    }
}
