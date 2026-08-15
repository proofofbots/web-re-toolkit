use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Operand {
    Int { value: i64 },
    Float { value: f64 },
    Text { value: String },
    Bool { value: bool },
    Null,
    Undefined,
    Register { index: u32 },
    Stack { depth: u32 },
    Address { pc: usize },
    Scope { depth: u32, slot: u32 },
    Opaque { note: String },
}

impl Operand {
    pub fn render(&self) -> String {
        match self {
            Operand::Int { value } => value.to_string(),
            Operand::Float { value } => format!("{value}"),
            Operand::Text { value } => render_string(value),
            Operand::Bool { value } => value.to_string(),
            Operand::Null => "null".to_string(),
            Operand::Undefined => "undefined".to_string(),
            Operand::Register { index } => format!("r{index}"),
            Operand::Stack { depth } => format!("stack[-{depth}]"),
            Operand::Address { pc } => format!("@{pc}"),
            Operand::Scope { depth, slot } => format!("scope[{depth}][{slot}]"),
            Operand::Opaque { note } => format!("<{note}>"),
        }
    }

    pub fn as_address(&self) -> Option<usize> {
        match self {
            Operand::Address { pc } => Some(*pc),
            Operand::Int { value } if *value >= 0 => Some(*value as usize),
            _ => None,
        }
    }
}

pub fn render_string(value: &str) -> String {
    let shown = if value.chars().count() > 60 {
        let clipped: String = value.chars().take(60).collect();
        format!("{clipped}…")
    } else {
        value.to_string()
    };

    serde_json::to_string(&shown).unwrap_or_else(|_| format!("\"{shown}\""))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum OpKind {
    Move,
    Binary { operator: String },
    Unary { operator: String },
    LoadConst,
    LoadProp,
    StoreProp,
    Call,
    New,
    MakeClosure,
    MakeObject,
    MakeArray,
    Push,
    Pop,
    Dup,
    Swap,
    Jump,
    Branch,
    Return,
    Throw,
    Halt,
    Nop,
    Unknown,
}

impl OpKind {
    pub fn terminates(&self) -> bool {
        matches!(self, OpKind::Return | OpKind::Throw | OpKind::Halt | OpKind::Jump)
    }

    pub fn touches_stack(&self) -> bool {
        matches!(self, OpKind::Push | OpKind::Pop | OpKind::Dup | OpKind::Swap)
    }

    pub fn stack_delta(&self) -> i32 {
        match self {
            OpKind::Push | OpKind::Dup => 1,
            OpKind::Pop => -1,
            _ => 0,
        }
    }

    pub fn name(&self) -> String {
        match self {
            OpKind::Binary { operator } => format!("binary {operator}"),
            OpKind::Unary { operator } => format!("unary {operator}"),
            other => format!("{other:?}").to_lowercase(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Guarded {
    #[default]
    Catch,
    Finally,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    pub start: usize,
    pub end: usize,
    pub target: usize,
    #[serde(default)]
    pub kind: Guarded,
}

impl Region {
    pub fn new(start: usize, end: usize, target: usize) -> Self {
        Self { start, end, target, kind: Guarded::Catch }
    }

    pub fn finally(start: usize, end: usize, target: usize) -> Self {
        Self { start, end, target, kind: Guarded::Finally }
    }

    pub fn covers(&self, pc: usize) -> bool {
        pc >= self.start && pc < self.end
    }

    pub fn render(&self) -> String {
        let word = match self.kind {
            Guarded::Catch => "catch",
            Guarded::Finally => "finally",
        };
        format!("@{}..@{} {word} @{}", self.start, self.end, self.target)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JumpTarget {
    pub pc: usize,
    #[serde(default)]
    pub taken_when: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instruction {
    pub pc: usize,
    pub opcode: u32,
    pub next: usize,
    #[serde(default)]
    pub operands: Vec<Operand>,
    #[serde(default)]
    pub writes: Vec<u32>,
    #[serde(default)]
    pub jumps: Vec<JumpTarget>,
    #[serde(default)]
    pub conditional: bool,
    #[serde(default)]
    pub terminal: bool,
    #[serde(default)]
    pub kind: OpKind,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

impl Default for OpKind {
    fn default() -> Self {
        OpKind::Unknown
    }
}

impl Instruction {
    pub fn new(pc: usize, opcode: u32, next: usize) -> Self {
        Self {
            pc,
            opcode,
            next,
            operands: Vec::new(),
            writes: Vec::new(),
            jumps: Vec::new(),
            conditional: false,
            terminal: false,
            kind: OpKind::Unknown,
            label: None,
            note: None,
        }
    }

    pub fn successors(&self) -> Vec<usize> {
        let mut out = Vec::new();

        for jump in &self.jumps {
            if !out.contains(&jump.pc) {
                out.push(jump.pc);
            }
        }

        if !self.terminal && !out.contains(&self.next) {
            out.push(self.next);
        }

        out
    }

    pub fn render(&self) -> String {
        let mut parts = Vec::new();

        if !self.writes.is_empty() {
            let targets: Vec<String> = self.writes.iter().map(|index| format!("r{index}")).collect();
            parts.push(format!("{} =", targets.join(", ")));
        }

        parts.push(
            self.label
                .clone()
                .unwrap_or_else(|| format!("op{}", self.opcode)),
        );

        if !self.operands.is_empty() {
            let rendered: Vec<String> = self.operands.iter().map(Operand::render).collect();
            parts.push(rendered.join(", "));
        }

        if !self.jumps.is_empty() {
            let targets: Vec<String> = self
                .jumps
                .iter()
                .map(|jump| match jump.taken_when {
                    Some(true) => format!("if-true @{}", jump.pc),
                    Some(false) => format!("if-false @{}", jump.pc),
                    None => format!("-> @{}", jump.pc),
                })
                .collect();
            parts.push(targets.join(" "));
        }

        parts.join(" ")
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VmProgram {
    pub instructions: BTreeMap<usize, Instruction>,
    #[serde(default)]
    pub entry: usize,
    #[serde(default)]
    pub pool: Vec<String>,
    #[serde(default)]
    pub handler_labels: BTreeMap<u32, String>,
    #[serde(default)]
    pub regions: Vec<Region>,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl VmProgram {
    pub fn from_instructions(instructions: Vec<Instruction>) -> Self {
        let mut map = BTreeMap::new();
        let entry = instructions.first().map(|entry| entry.pc).unwrap_or(0);

        for instruction in instructions {
            map.insert(instruction.pc, instruction);
        }

        Self {
            instructions: map,
            entry,
            pool: Vec::new(),
            handler_labels: BTreeMap::new(),
            regions: Vec::new(),
            notes: Vec::new(),
        }
    }

    pub fn guarding(&self, pc: usize) -> Vec<&Region> {
        self.regions
            .iter()
            .filter(|region| region.covers(pc))
            .collect()
    }

    pub fn handler_targets(&self, pc: usize) -> Vec<usize> {
        self.guarding(pc).into_iter().map(|region| region.target).collect()
    }

    pub fn successors_of(&self, pc: usize, instruction: &Instruction) -> Vec<usize> {
        let mut out = instruction.successors();

        for target in self.handler_targets(pc) {
            if !out.contains(&target) {
                out.push(target);
            }
        }

        out
    }

    pub fn get(&self, pc: usize) -> Option<&Instruction> {
        self.instructions.get(&pc)
    }

    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }

    pub fn addresses(&self) -> Vec<usize> {
        self.instructions.keys().copied().collect()
    }

    pub fn reachable(&self, entry: usize) -> BTreeSet<usize> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![entry];

        while let Some(pc) = stack.pop() {
            if !seen.insert(pc) {
                continue;
            }

            let Some(instruction) = self.get(pc) else {
                continue;
            };

            for successor in self.successors_of(pc, instruction) {
                if self.instructions.contains_key(&successor) {
                    stack.push(successor);
                }
            }
        }

        seen
    }

    pub fn jump_targets(&self) -> BTreeSet<usize> {
        let mut out = BTreeSet::new();

        for instruction in self.instructions.values() {
            for jump in &instruction.jumps {
                out.insert(jump.pc);
            }
        }

        out
    }

    pub fn callers_of(&self, pc: usize) -> Vec<usize> {
        self.instructions
            .values()
            .filter(|instruction| instruction.successors().contains(&pc))
            .map(|instruction| instruction.pc)
            .collect()
    }

    pub fn listing(&self) -> String {
        let mut out = String::new();
        let targets = self.jump_targets();

        for instruction in self.instructions.values() {
            if targets.contains(&instruction.pc) {
                out.push_str(&format!("\n@{}:\n", instruction.pc));
            }
            out.push_str(&format!("  {:>6}  {}\n", instruction.pc, instruction.render()));
        }

        out
    }

    pub fn strings(&self) -> Vec<String> {
        let mut out = BTreeSet::new();

        for instruction in self.instructions.values() {
            for operand in &instruction.operands {
                if let Operand::Text { value } = operand {
                    out.insert(value.clone());
                }
            }
        }

        out.into_iter().collect()
    }

    pub fn opcode_histogram(&self) -> BTreeMap<u32, usize> {
        let mut out = BTreeMap::new();
        for instruction in self.instructions.values() {
            *out.entry(instruction.opcode).or_insert(0) += 1;
        }
        out
    }

    pub fn validate(&self) -> Result<()> {
        if self.instructions.is_empty() {
            return Err(Error::msg("instruction stream is empty"));
        }

        for instruction in self.instructions.values() {
            for jump in &instruction.jumps {
                if !self.instructions.contains_key(&jump.pc) {
                    return Err(Error::msg(format!(
                        "instruction at {} jumps to {} which is not decoded",
                        instruction.pc, jump.pc
                    )));
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionRange {
    pub entry: usize,
    pub addresses: Vec<usize>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub calls: Vec<usize>,
}

pub fn carve_functions(program: &VmProgram) -> Vec<FunctionRange> {
    let mut entries: BTreeSet<usize> = BTreeSet::new();
    entries.insert(program.entry);

    for instruction in program.instructions.values() {
        if matches!(instruction.kind, OpKind::MakeClosure) {
            for operand in &instruction.operands {
                if let Some(pc) = operand.as_address() {
                    if program.instructions.contains_key(&pc) {
                        entries.insert(pc);
                    }
                }
            }
        }
    }

    let mut out = Vec::new();

    for entry in entries {
        let addresses = program.reachable(entry);
        let calls = addresses
            .iter()
            .filter_map(|pc| program.get(*pc))
            .filter(|instruction| matches!(instruction.kind, OpKind::MakeClosure))
            .flat_map(|instruction| {
                instruction
                    .operands
                    .iter()
                    .filter_map(Operand::as_address)
                    .collect::<Vec<_>>()
            })
            .collect();

        out.push(FunctionRange {
            entry,
            addresses: addresses.into_iter().collect(),
            name: None,
            calls,
        });
    }

    out
}

#[cfg(test)]
mod region_tests {
    use super::*;

    fn program() -> VmProgram {
        let mut instructions = Vec::new();
        for pc in 0..6 {
            let mut instruction = Instruction::new(pc, pc as u32, pc + 1);
            instruction.kind = OpKind::Nop;
            instructions.push(instruction);
        }

        let mut last = Instruction::new(6, 6, 7);
        last.kind = OpKind::Return;
        last.terminal = true;
        instructions.push(last);

        let mut program = VmProgram::from_instructions(instructions);
        program.regions.push(Region::new(1, 4, 6));
        program
    }

    #[test]
    fn a_region_covers_a_half_open_range() {
        let region = Region::new(1, 4, 6);

        assert!(!region.covers(0));
        assert!(region.covers(1));
        assert!(region.covers(3));
        assert!(!region.covers(4));
    }

    #[test]
    fn only_guarded_addresses_reach_the_handler() {
        let program = program();

        assert_eq!(program.handler_targets(2), vec![6]);
        assert!(program.handler_targets(5).is_empty());
        assert_eq!(program.guarding(2).len(), 1);
    }

    #[test]
    fn a_guarded_instruction_gains_the_handler_as_a_successor() {
        let program = program();
        let instruction = program.get(2).unwrap();

        assert_eq!(instruction.successors(), vec![3]);
        assert_eq!(program.successors_of(2, instruction), vec![3, 6]);
    }

    #[test]
    fn nested_regions_both_apply() {
        let mut program = program();
        program.regions.push(Region::finally(2, 3, 5));

        let targets = program.handler_targets(2);
        assert!(targets.contains(&6));
        assert!(targets.contains(&5));
    }

    #[test]
    fn a_region_describes_itself() {
        assert_eq!(Region::new(1, 4, 9).render(), "@1..@4 catch @9");
        assert_eq!(Region::finally(1, 4, 9).render(), "@1..@4 finally @9");
    }

    #[test]
    fn stack_operations_declare_their_effect_on_depth() {
        assert_eq!(OpKind::Push.stack_delta(), 1);
        assert_eq!(OpKind::Dup.stack_delta(), 1);
        assert_eq!(OpKind::Pop.stack_delta(), -1);
        assert_eq!(OpKind::Swap.stack_delta(), 0);
        assert_eq!(OpKind::Nop.stack_delta(), 0);

        assert!(OpKind::Push.touches_stack());
        assert!(!OpKind::Move.touches_stack());
    }

    #[test]
    fn a_stack_operand_renders_by_depth() {
        assert_eq!(Operand::Stack { depth: 2 }.render(), "stack[-2]");
    }

    #[test]
    fn a_program_with_regions_round_trips_through_json() {
        let program = program();
        let text = serde_json::to_string(&program).unwrap();
        let back: VmProgram = serde_json::from_str(&text).unwrap();

        assert_eq!(back.regions, program.regions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight_line() -> VmProgram {
        let mut first = Instruction::new(0, 1, 1);
        first.writes = vec![0];
        first.operands = vec![Operand::Int { value: 7 }];
        first.kind = OpKind::LoadConst;

        let mut second = Instruction::new(1, 2, 2);
        second.jumps = vec![JumpTarget { pc: 3, taken_when: Some(true) }];
        second.conditional = true;
        second.kind = OpKind::Branch;

        let mut third = Instruction::new(2, 3, 3);
        third.kind = OpKind::Move;

        let mut fourth = Instruction::new(3, 4, 4);
        fourth.terminal = true;
        fourth.kind = OpKind::Return;

        VmProgram::from_instructions(vec![first, second, third, fourth])
    }

    #[test]
    fn walks_successors_and_reachability() {
        let program = straight_line();
        assert_eq!(program.get(1).unwrap().successors(), vec![3, 2]);
        assert_eq!(program.reachable(0).len(), 4);
        assert!(program.validate().is_ok());
    }

    #[test]
    fn rejects_dangling_jumps() {
        let mut program = straight_line();
        program
            .instructions
            .get_mut(&1)
            .unwrap()
            .jumps
            .push(JumpTarget { pc: 99, taken_when: None });
        assert!(program.validate().is_err());
    }

    #[test]
    fn renders_a_listing() {
        let listing = straight_line().listing();
        assert!(listing.contains("@3:"));
        assert!(listing.contains("r0 ="));
    }
}
