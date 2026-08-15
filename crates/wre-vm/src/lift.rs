use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::cfg::Cfg;
use crate::ir::{Instruction, OpKind, Operand, VmProgram};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LiftMode {
    #[default]
    Structured,
    Dispatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiftOptions {
    pub mode: LiftMode,
    pub register_prefix: String,
    pub function_prefix: String,
    pub annotate: bool,
    pub max_depth: usize,
}

impl Default for LiftOptions {
    fn default() -> Self {
        Self {
            mode: LiftMode::Structured,
            register_prefix: "r".to_string(),
            function_prefix: "vm".to_string(),
            annotate: false,
            max_depth: 64,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LiftReport {
    pub functions: usize,
    pub structured: usize,
    pub dispatched: usize,
    pub unknown_opcodes: BTreeSet<u32>,
}

pub struct Lifter<'p> {
    program: &'p VmProgram,
    options: LiftOptions,
    report: LiftReport,
}

impl<'p> Lifter<'p> {
    pub fn new(program: &'p VmProgram, options: LiftOptions) -> Self {
        Self { program, options, report: LiftReport::default() }
    }

    pub fn report(&self) -> &LiftReport {
        &self.report
    }

    pub fn lift_all(&mut self, entries: &[usize]) -> String {
        let mut out = String::new();

        for entry in entries {
            out.push_str(&self.lift_function(*entry));
            out.push_str("\n\n");
        }

        out
    }

    pub fn lift_function(&mut self, entry: usize) -> String {
        self.report.functions += 1;

        let cfg = Cfg::build(self.program, entry);
        if cfg.is_empty() {
            return format!(
                "function {}_{entry}() {{\n  return undefined;\n}}",
                self.options.function_prefix
            );
        }

        let registers = self.registers_used(&cfg);
        let mut declaration = if registers.is_empty() {
            String::new()
        } else {
            let names: Vec<String> = registers
                .iter()
                .map(|index| format!("{}{index}", self.options.register_prefix))
                .collect();
            format!("  let {};\n", names.join(", "))
        };

        if self.uses_stack(&cfg) {
            declaration.push_str("  const stack = [];\n");
        }

        let body = if self.options.mode == LiftMode::Dispatch || !cfg.is_reducible() {
            self.report.dispatched += 1;
            self.emit_dispatch(&cfg)
        } else {
            let mut emitter = Structured::new(self, &cfg);
            match emitter.run() {
                Some(text) => {
                    self.report.structured += 1;
                    text
                }
                None => {
                    self.report.dispatched += 1;
                    self.emit_dispatch(&cfg)
                }
            }
        };

        format!(
            "function {}_{entry}() {{\n{declaration}{body}}}",
            self.options.function_prefix
        )
    }

    fn uses_stack(&self, cfg: &Cfg) -> bool {
        cfg.blocks.iter().any(|block| {
            block.addresses.iter().any(|pc| {
                self.program
                    .get(*pc)
                    .is_some_and(|instruction| instruction.kind.touches_stack())
            })
        })
    }

    fn registers_used(&self, cfg: &Cfg) -> BTreeSet<u32> {
        let mut out = BTreeSet::new();

        for block in &cfg.blocks {
            for pc in &block.addresses {
                let Some(instruction) = self.program.get(*pc) else {
                    continue;
                };
                for index in &instruction.writes {
                    out.insert(*index);
                }
                for operand in &instruction.operands {
                    if let Operand::Register { index } = operand {
                        out.insert(*index);
                    }
                }
            }
        }

        out
    }

    fn emit_dispatch(&mut self, cfg: &Cfg) -> String {
        let mut out = String::new();
        out.push_str(&format!("  let pc = {};\n", cfg.entry));
        out.push_str("  dispatch: while (true) {\n    switch (pc) {\n");

        for block in &cfg.blocks {
            out.push_str(&format!("      case {}: {{\n", block.start));

            for pc in &block.addresses {
                let Some(instruction) = self.program.get(*pc) else {
                    continue;
                };
                for line in self.render(instruction) {
                    out.push_str(&format!("        {line}\n"));
                }
            }

            let last = block.addresses.last().and_then(|pc| self.program.get(*pc));

            match last {
                Some(instruction) if instruction.terminal && instruction.jumps.is_empty() => {
                    if !matches!(instruction.kind, OpKind::Return | OpKind::Throw) {
                        out.push_str("        break dispatch;\n");
                    }
                }
                Some(instruction) if instruction.conditional && !instruction.jumps.is_empty() => {
                    let test = self.condition(instruction);
                    let taken = instruction.jumps[0].pc;
                    let fallthrough = instruction.next;
                    let when_true = instruction.jumps[0].taken_when.unwrap_or(true);
                    let (yes, no) = if when_true { (taken, fallthrough) } else { (fallthrough, taken) };
                    out.push_str(&format!("        pc = ({test}) ? {yes} : {no};\n"));
                    out.push_str("        continue dispatch;\n");
                }
                Some(instruction) if !instruction.jumps.is_empty() => {
                    out.push_str(&format!("        pc = {};\n", instruction.jumps[0].pc));
                    out.push_str("        continue dispatch;\n");
                }
                Some(instruction) => {
                    out.push_str(&format!("        pc = {};\n", instruction.next));
                    out.push_str("        continue dispatch;\n");
                }
                None => out.push_str("        break dispatch;\n"),
            }

            out.push_str("      }\n");
        }

        out.push_str("      default: break dispatch;\n");
        out.push_str("    }\n  }\n");
        out
    }

    fn condition(&self, instruction: &Instruction) -> String {
        instruction
            .operands
            .first()
            .map(Operand::render)
            .unwrap_or_else(|| format!("op{}Test", instruction.opcode))
    }

    fn render(&mut self, instruction: &Instruction) -> Vec<String> {
        let register = |index: &u32| format!("{}{index}", self.options.register_prefix);
        let target = instruction.writes.first().map(&register);
        let operands: Vec<String> = instruction.operands.iter().map(Operand::render).collect();

        let mut lines = Vec::new();

        if self.options.annotate {
            lines.push(format!("// {:>6}  {}", instruction.pc, instruction.render()));
        }

        let assign = |value: String| match &target {
            Some(name) => format!("{name} = {value};"),
            None => format!("{value};"),
        };

        let statement = match &instruction.kind {
            OpKind::Nop => None,
            OpKind::Jump | OpKind::Branch => None,
            OpKind::Push => {
                let value = operands.first().cloned().unwrap_or_else(|| "undefined".into());
                Some(format!("stack.push({value});"))
            }
            OpKind::Pop => Some(assign("stack.pop()".to_string())),
            OpKind::Dup => Some("stack.push(stack[stack.length - 1]);".to_string()),
            OpKind::Swap => Some(
                "stack.splice(stack.length - 2, 2, stack[stack.length - 1], stack[stack.length - 2]);"
                    .to_string(),
            ),
            OpKind::LoadConst | OpKind::Move => {
                Some(assign(operands.first().cloned().unwrap_or_else(|| "undefined".into())))
            }
            OpKind::Binary { operator } => {
                let left = operands.first().cloned().unwrap_or_else(|| "undefined".into());
                let right = operands.get(1).cloned().unwrap_or_else(|| "undefined".into());
                Some(assign(format!("{left} {operator} {right}")))
            }
            OpKind::Unary { operator } => {
                let value = operands.first().cloned().unwrap_or_else(|| "undefined".into());
                Some(assign(format!("{operator}{value}")))
            }
            OpKind::LoadProp => {
                let object = operands.first().cloned().unwrap_or_else(|| "undefined".into());
                let key = operands.get(1).cloned().unwrap_or_else(|| "undefined".into());
                Some(assign(format!("{object}[{key}]")))
            }
            OpKind::StoreProp => {
                let object = operands.first().cloned().unwrap_or_else(|| "undefined".into());
                let key = operands.get(1).cloned().unwrap_or_else(|| "undefined".into());
                let value = operands.get(2).cloned().unwrap_or_else(|| "undefined".into());
                Some(format!("{object}[{key}] = {value};"))
            }
            OpKind::Call => {
                let callee = operands.first().cloned().unwrap_or_else(|| "undefined".into());
                let arguments = operands.iter().skip(1).cloned().collect::<Vec<_>>().join(", ");
                Some(assign(format!("{callee}({arguments})")))
            }
            OpKind::New => {
                let callee = operands.first().cloned().unwrap_or_else(|| "undefined".into());
                let arguments = operands.iter().skip(1).cloned().collect::<Vec<_>>().join(", ");
                Some(assign(format!("new {callee}({arguments})")))
            }
            OpKind::MakeClosure => {
                let address = instruction
                    .operands
                    .iter()
                    .find_map(Operand::as_address)
                    .map(|pc| format!("{}_{pc}", self.options.function_prefix))
                    .unwrap_or_else(|| "function () {}".to_string());
                Some(assign(address))
            }
            OpKind::MakeObject => Some(assign("{}".to_string())),
            OpKind::MakeArray => Some(assign(format!("[{}]", operands.join(", ")))),
            OpKind::Return => Some(format!(
                "return {};",
                operands.first().cloned().unwrap_or_else(|| "undefined".into())
            )),
            OpKind::Throw => Some(format!(
                "throw {};",
                operands.first().cloned().unwrap_or_else(|| "undefined".into())
            )),
            OpKind::Halt => Some("return;".to_string()),
            OpKind::Unknown => {
                self.report.unknown_opcodes.insert(instruction.opcode);
                let name = instruction
                    .label
                    .clone()
                    .unwrap_or_else(|| format!("op{}", instruction.opcode));
                Some(assign(format!("{name}({})", operands.join(", "))))
            }
        };

        if let Some(statement) = statement {
            lines.push(statement);
        }

        lines
    }
}

struct Structured<'l, 'p> {
    lifter: &'l mut Lifter<'p>,
    cfg: &'l Cfg,
    ipdom: Vec<Option<usize>>,
    loop_headers: BTreeMap<usize, BTreeSet<usize>>,
    emitted: BTreeSet<usize>,
    open_loops: Vec<usize>,
    failed: bool,
}

impl<'l, 'p> Structured<'l, 'p> {
    fn new(lifter: &'l mut Lifter<'p>, cfg: &'l Cfg) -> Self {
        let ipdom = cfg.post_dominators();

        let mut loop_headers = BTreeMap::new();
        for info in cfg.loops() {
            loop_headers.insert(info.header, info.body.clone());
        }

        Self {
            lifter,
            cfg,
            ipdom,
            loop_headers,
            emitted: BTreeSet::new(),
            open_loops: Vec::new(),
            failed: false,
        }
    }

    fn run(&mut self) -> Option<String> {
        let entry = self.cfg.entry_block()?;
        let mut out = String::new();
        self.emit(entry, None, 1, &mut out);

        if self.failed { None } else { Some(out) }
    }

    fn indent(depth: usize) -> String {
        "  ".repeat(depth)
    }

    fn emit(&mut self, block: usize, stop: Option<usize>, depth: usize, out: &mut String) {
        if self.failed || depth > self.lifter.options.max_depth {
            self.failed = true;
            return;
        }

        if Some(block) == stop {
            return;
        }

        if self.open_loops.contains(&block) {
            out.push_str(&format!("{}continue loop_{block};\n", Self::indent(depth)));
            return;
        }

        if !self.emitted.insert(block) {
            self.failed = true;
            return;
        }

        let is_loop_header = self.loop_headers.contains_key(&block);

        if is_loop_header {
            out.push_str(&format!("{}loop_{block}: while (true) {{\n", Self::indent(depth)));
            self.open_loops.push(block);
            self.emit_body(block, stop, depth + 1, out);
            self.open_loops.pop();
            out.push_str(&format!("{}break loop_{block};\n", Self::indent(depth + 1)));
            out.push_str(&format!("{}}}\n", Self::indent(depth)));
            return;
        }

        self.emit_body(block, stop, depth, out);
    }

    fn emit_body(&mut self, block: usize, stop: Option<usize>, depth: usize, out: &mut String) {
        let padding = Self::indent(depth);
        let addresses = self.cfg.blocks[block].addresses.clone();

        for pc in &addresses {
            let Some(instruction) = self.lifter.program.get(*pc).cloned() else {
                continue;
            };
            for line in self.lifter.render(&instruction) {
                out.push_str(&format!("{padding}{line}\n"));
            }
        }

        let successors = self.cfg.blocks[block].successors.clone();

        match successors.len() {
            0 => {}
            1 => self.emit_edge(successors[0], stop, depth, out),
            2 => {
                let Some(last) = self.cfg.blocks[block]
                    .addresses
                    .last()
                    .and_then(|pc| self.lifter.program.get(*pc))
                    .cloned()
                else {
                    self.failed = true;
                    return;
                };

                let test = self.lifter.condition(&last);
                let taken = last.jumps.first().map(|jump| jump.pc);
                let taken_block = taken.and_then(|pc| self.cfg.by_address.get(&pc).copied());
                let when_true = last
                    .jumps
                    .first()
                    .and_then(|jump| jump.taken_when)
                    .unwrap_or(true);

                let fallthrough = successors
                    .iter()
                    .copied()
                    .find(|id| Some(*id) != taken_block)
                    .unwrap_or(successors[0]);

                let (yes, no) = match taken_block {
                    Some(target) if when_true => (target, fallthrough),
                    Some(target) => (fallthrough, target),
                    None => (successors[0], successors[1]),
                };

                let join = self.ipdom.get(block).copied().flatten().filter(|id| *id != block);

                let mut consequent = String::new();
                self.emit_branch(yes, join, stop, depth + 1, &mut consequent);

                let mut alternate = String::new();
                self.emit_branch(no, join, stop, depth + 1, &mut alternate);

                if consequent.trim().is_empty() && !alternate.trim().is_empty() {
                    out.push_str(&format!("{padding}if (!({test})) {{\n"));
                    out.push_str(&alternate);
                    out.push_str(&format!("{padding}}}\n"));
                } else {
                    out.push_str(&format!("{padding}if ({test}) {{\n"));
                    out.push_str(&consequent);

                    if alternate.trim().is_empty() {
                        out.push_str(&format!("{padding}}}\n"));
                    } else {
                        out.push_str(&format!("{padding}}} else {{\n"));
                        out.push_str(&alternate);
                        out.push_str(&format!("{padding}}}\n"));
                    }
                }

                if let Some(join) = join {
                    if Some(join) != stop {
                        self.emit_edge(join, stop, depth, out);
                    }
                }
            }
            _ => self.failed = true,
        }
    }

    fn emit_branch(
        &mut self,
        block: usize,
        join: Option<usize>,
        stop: Option<usize>,
        depth: usize,
        out: &mut String,
    ) {
        if Some(block) == join {
            return;
        }
        self.emit_edge_inner(block, join.or(stop), depth, out);
    }

    fn emit_edge(&mut self, block: usize, stop: Option<usize>, depth: usize, out: &mut String) {
        self.emit_edge_inner(block, stop, depth, out);
    }

    fn emit_edge_inner(
        &mut self,
        block: usize,
        stop: Option<usize>,
        depth: usize,
        out: &mut String,
    ) {
        if Some(block) == stop {
            return;
        }

        if self.open_loops.contains(&block) {
            out.push_str(&format!("{}continue loop_{block};\n", Self::indent(depth)));
            return;
        }

        if self.emitted.contains(&block) {
            if let Some(header) = self.open_loops.last().copied() {
                if !self
                    .loop_headers
                    .get(&header)
                    .map(|body| body.contains(&block))
                    .unwrap_or(false)
                {
                    out.push_str(&format!("{}break loop_{header};\n", Self::indent(depth)));
                    return;
                }
            }
            self.failed = true;
            return;
        }

        self.emit(block, stop, depth, out);
    }
}

pub fn lift(program: &VmProgram, entries: &[usize], options: LiftOptions) -> (String, LiftReport) {
    let mut lifter = Lifter::new(program, options);
    let code = lifter.lift_all(entries);
    let report = lifter.report().clone();
    (code, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Instruction, JumpTarget};

    fn branch_program() -> VmProgram {
        let mut load = Instruction::new(0, 1, 1);
        load.kind = OpKind::LoadConst;
        load.writes = vec![0];
        load.operands = vec![Operand::Int { value: 3 }];

        let mut test = Instruction::new(1, 2, 2);
        test.kind = OpKind::Branch;
        test.conditional = true;
        test.operands = vec![Operand::Register { index: 0 }];
        test.jumps = vec![JumpTarget { pc: 3, taken_when: Some(true) }];

        let mut left = Instruction::new(2, 3, 4);
        left.kind = OpKind::LoadConst;
        left.writes = vec![1];
        left.operands = vec![Operand::Text { value: "low".into() }];

        let mut right = Instruction::new(3, 4, 4);
        right.kind = OpKind::LoadConst;
        right.writes = vec![1];
        right.operands = vec![Operand::Text { value: "high".into() }];

        let mut done = Instruction::new(4, 5, 5);
        done.kind = OpKind::Return;
        done.terminal = true;
        done.operands = vec![Operand::Register { index: 1 }];

        VmProgram::from_instructions(vec![load, test, left, right, done])
    }

    #[test]
    fn lifts_a_branch_to_if_else() {
        let program = branch_program();
        let (code, report) = lift(&program, &[0], LiftOptions::default());

        assert!(code.contains("if (r0)"), "{code}");
        assert!(code.contains("\"high\""), "{code}");
        assert!(code.contains("\"low\""), "{code}");
        assert!(code.contains("return r1;"), "{code}");
        assert_eq!(report.structured, 1);
        assert_eq!(report.dispatched, 0);
    }

    #[test]
    fn dispatch_mode_emits_a_switch() {
        let program = branch_program();
        let options = LiftOptions { mode: LiftMode::Dispatch, ..LiftOptions::default() };
        let (code, report) = lift(&program, &[0], options);

        assert!(code.contains("switch (pc)"), "{code}");
        assert!(code.contains("continue dispatch;"), "{code}");
        assert_eq!(report.dispatched, 1);
    }

    #[test]
    fn lifts_a_loop_to_while() {
        let mut init = Instruction::new(0, 1, 1);
        init.kind = OpKind::LoadConst;
        init.writes = vec![0];
        init.operands = vec![Operand::Int { value: 0 }];

        let mut test = Instruction::new(1, 2, 2);
        test.kind = OpKind::Branch;
        test.conditional = true;
        test.operands = vec![Operand::Register { index: 0 }];
        test.jumps = vec![JumpTarget { pc: 4, taken_when: Some(false) }];

        let mut step = Instruction::new(2, 3, 3);
        step.kind = OpKind::Binary { operator: "+".into() };
        step.writes = vec![0];
        step.operands = vec![Operand::Register { index: 0 }, Operand::Int { value: 1 }];

        let mut back = Instruction::new(3, 4, 4);
        back.kind = OpKind::Jump;
        back.terminal = true;
        back.jumps = vec![JumpTarget { pc: 1, taken_when: None }];

        let mut done = Instruction::new(4, 5, 5);
        done.kind = OpKind::Return;
        done.terminal = true;
        done.operands = vec![Operand::Register { index: 0 }];

        let program = VmProgram::from_instructions(vec![init, test, step, back, done]);
        let (code, _) = lift(&program, &[0], LiftOptions::default());

        assert!(code.contains("while (true)"), "{code}");
        assert!(code.contains("continue loop_"), "{code}");
        assert!(code.contains("r0 = r0 + 1;"), "{code}");
    }

    #[test]
    fn unknown_opcodes_are_reported() {
        let mut odd = Instruction::new(0, 77, 1);
        odd.writes = vec![2];
        odd.operands = vec![Operand::Register { index: 1 }];

        let mut done = Instruction::new(1, 5, 2);
        done.kind = OpKind::Return;
        done.terminal = true;

        let program = VmProgram::from_instructions(vec![odd, done]);
        let (code, report) = lift(&program, &[0], LiftOptions::default());

        assert!(code.contains("op77(r1)"), "{code}");
        assert!(report.unknown_opcodes.contains(&77));
    }
}
