use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use wre_core::digest::sha256_short;

use crate::ir::VmProgram;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    pub pc: usize,
    pub opcode: u32,
    #[serde(default)]
    pub handler: Option<String>,
    #[serde(default)]
    pub frame: Option<String>,
    #[serde(default)]
    pub sequence: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpcodeMap {
    pub by_opcode: BTreeMap<u32, String>,
    pub by_handler: BTreeMap<String, Vec<u32>>,
    pub conflicts: Vec<u32>,
    pub samples: BTreeMap<u32, usize>,
}

impl OpcodeMap {
    pub fn identity_of(&self, opcode: u32) -> Option<&str> {
        self.by_opcode.get(&opcode).map(String::as_str)
    }

    pub fn opcodes_for(&self, handler: &str) -> &[u32] {
        self.by_handler
            .get(handler)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn coverage(&self) -> usize {
        self.by_opcode.len()
    }
}

pub fn handler_identity(source: &str) -> String {
    let normalised: String = source.split_whitespace().collect::<Vec<_>>().join(" ");
    sha256_short(normalised.as_bytes())
}

pub fn align(trace: &[TraceEntry]) -> OpcodeMap {
    let mut map = OpcodeMap::default();
    let mut conflicts = BTreeSet::new();

    for entry in trace {
        *map.samples.entry(entry.opcode).or_insert(0) += 1;

        let Some(handler) = &entry.handler else {
            continue;
        };

        match map.by_opcode.get(&entry.opcode) {
            None => {
                map.by_opcode.insert(entry.opcode, handler.clone());
            }
            Some(existing) if existing == handler => {}
            Some(_) => {
                conflicts.insert(entry.opcode);
            }
        }
    }

    for (opcode, handler) in &map.by_opcode {
        map.by_handler
            .entry(handler.clone())
            .or_default()
            .push(*opcode);
    }

    map.conflicts = conflicts.into_iter().collect();
    map
}

pub fn from_sources(sources: &[String]) -> OpcodeMap {
    let mut map = OpcodeMap::default();

    for (index, source) in sources.iter().enumerate() {
        if source.is_empty() {
            continue;
        }
        let identity = handler_identity(source);
        map.by_opcode.insert(index as u32, identity.clone());
        map.by_handler.entry(identity).or_default().push(index as u32);
    }

    map
}

pub fn permutation(from: &OpcodeMap, to: &OpcodeMap) -> BTreeMap<u32, u32> {
    let mut out = BTreeMap::new();

    for (opcode, handler) in &from.by_opcode {
        let candidates = to.opcodes_for(handler);
        if candidates.len() == 1 {
            out.insert(*opcode, candidates[0]);
        }
    }

    out
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Coverage {
    pub visited: BTreeSet<usize>,
    pub decoded: usize,
    pub hits: usize,
    pub unreached: Vec<usize>,
}

impl Coverage {
    pub fn ratio(&self) -> f64 {
        if self.decoded == 0 {
            return 0.0;
        }
        self.visited.len() as f64 / self.decoded as f64
    }
}

pub fn coverage(program: &VmProgram, trace: &[TraceEntry]) -> Coverage {
    let visited: BTreeSet<usize> = trace.iter().map(|entry| entry.pc).collect();
    let decoded = program.len();

    let unreached = program
        .addresses()
        .into_iter()
        .filter(|pc| !visited.contains(pc))
        .collect();

    Coverage {
        hits: trace.len(),
        visited,
        decoded,
        unreached,
    }
}

pub fn label_program(program: &mut VmProgram, labels: &BTreeMap<u32, String>) -> usize {
    let mut applied = 0usize;

    for instruction in program.instructions.values_mut() {
        if let Some(label) = labels.get(&instruction.opcode) {
            instruction.label = Some(label.clone());
            applied += 1;
        }
    }

    program.handler_labels = labels.clone();
    applied
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(pc: usize, opcode: u32, handler: &str) -> TraceEntry {
        TraceEntry {
            pc,
            opcode,
            handler: Some(handler.to_string()),
            frame: None,
            sequence: pc,
        }
    }

    #[test]
    fn aligns_opcodes_to_handlers() {
        let trace = vec![
            entry(0, 7, "aaa"),
            entry(1, 9, "bbb"),
            entry(2, 7, "aaa"),
        ];

        let map = align(&trace);
        assert_eq!(map.identity_of(7), Some("aaa"));
        assert_eq!(map.identity_of(9), Some("bbb"));
        assert!(map.conflicts.is_empty());
        assert_eq!(map.samples.get(&7), Some(&2));
    }

    #[test]
    fn reports_conflicting_opcodes() {
        let trace = vec![entry(0, 7, "aaa"), entry(1, 7, "ccc")];
        let map = align(&trace);
        assert_eq!(map.conflicts, vec![7]);
    }

    #[test]
    fn recovers_the_permutation_between_builds() {
        let old = from_sources(&[
            "function a() { return 1; }".to_string(),
            "function b() { return 2; }".to_string(),
            "function c() { return 3; }".to_string(),
        ]);

        let new = from_sources(&[
            "function c() { return 3; }".to_string(),
            "function a() { return 1; }".to_string(),
            "function b() { return 2; }".to_string(),
        ]);

        let mapping = permutation(&old, &new);
        assert_eq!(mapping.get(&0), Some(&1));
        assert_eq!(mapping.get(&1), Some(&2));
        assert_eq!(mapping.get(&2), Some(&0));
    }

    #[test]
    fn identity_ignores_whitespace() {
        let left = handler_identity("function (a,b) {\n  return a + b;\n}");
        let right = handler_identity("function (a,b) { return a + b; }");
        assert_eq!(left, right);
    }
}
