use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::ir::{Instruction, VmProgram};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub id: usize,
    pub start: usize,
    pub addresses: Vec<usize>,
    pub successors: Vec<usize>,
    pub predecessors: Vec<usize>,
    #[serde(default)]
    pub conditional: bool,
    #[serde(default)]
    pub terminal: bool,
}

impl Block {
    pub fn last<'p>(&self, program: &'p VmProgram) -> Option<&'p Instruction> {
        self.addresses.last().and_then(|pc| program.get(*pc))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cfg {
    pub entry: usize,
    pub blocks: Vec<Block>,
    pub by_address: BTreeMap<usize, usize>,
}

impl Cfg {
    pub fn build(program: &VmProgram, entry: usize) -> Self {
        let reachable = program.reachable(entry);
        if reachable.is_empty() {
            return Cfg::default();
        }

        let mut ordered: Vec<usize> = reachable.iter().copied().collect();
        ordered.sort_unstable();

        let following: BTreeMap<usize, usize> = ordered
            .windows(2)
            .map(|pair| (pair[0], pair[1]))
            .collect();

        let mut predecessors: BTreeMap<usize, usize> = BTreeMap::new();
        for pc in &ordered {
            let Some(instruction) = program.get(*pc) else {
                continue;
            };
            for successor in program.successors_of(*pc, instruction) {
                if reachable.contains(&successor) {
                    *predecessors.entry(successor).or_insert(0) += 1;
                }
            }
        }

        let mut leaders: BTreeSet<usize> = BTreeSet::new();
        leaders.insert(entry);

        for pc in &ordered {
            let Some(instruction) = program.get(*pc) else {
                continue;
            };

            let next_in_order = following.get(pc).copied();
            let successors = program.successors_of(*pc, instruction);

            for successor in &successors {
                if !reachable.contains(successor) {
                    continue;
                }

                if Some(*successor) != next_in_order {
                    leaders.insert(*successor);
                }

                if predecessors.get(successor).copied().unwrap_or(0) > 1 {
                    leaders.insert(*successor);
                }
            }

            let leaves_block = instruction.terminal
                || !instruction.jumps.is_empty()
                || successors.len() > 1
                || (successors.len() == 1 && Some(successors[0]) != next_in_order);

            if leaves_block {
                if let Some(next) = next_in_order {
                    leaders.insert(next);
                }
            }
        }

        let mut blocks: Vec<Block> = Vec::new();
        let mut by_address: BTreeMap<usize, usize> = BTreeMap::new();
        let mut current: Option<Block> = None;

        for pc in ordered {
            let Some(instruction) = program.get(pc) else {
                continue;
            };

            let starts_block = leaders.contains(&pc) || current.is_none();

            if starts_block {
                if let Some(block) = current.take() {
                    blocks.push(block);
                }
                current = Some(Block {
                    id: blocks.len(),
                    start: pc,
                    addresses: Vec::new(),
                    successors: Vec::new(),
                    predecessors: Vec::new(),
                    conditional: false,
                    terminal: false,
                });
            }

            let block = current.as_mut().expect("block open");
            by_address.insert(pc, block.id);
            block.addresses.push(pc);

            let successors = program.successors_of(pc, instruction);
            let next_in_order = following.get(&pc).copied();

            let ends_block = instruction.terminal
                || !instruction.jumps.is_empty()
                || successors.len() != 1
                || Some(successors[0]) != next_in_order
                || next_in_order.map(|next| leaders.contains(&next)).unwrap_or(true);

            if ends_block {
                block.conditional = instruction.conditional;
                block.terminal = instruction.terminal && instruction.jumps.is_empty();
                let block = current.take().expect("block open");
                blocks.push(block);
            }
        }

        if let Some(block) = current.take() {
            blocks.push(block);
        }

        let mut cfg = Cfg { entry, blocks, by_address };
        cfg.link(program);
        cfg
    }

    fn link(&mut self, program: &VmProgram) {
        let mut successors: Vec<Vec<usize>> = vec![Vec::new(); self.blocks.len()];

        for block in &self.blocks {
            let Some(last) = block.addresses.last().and_then(|pc| program.get(*pc)) else {
                continue;
            };

            for target in last.successors() {
                if let Some(id) = self.by_address.get(&target) {
                    if !successors[block.id].contains(id) {
                        successors[block.id].push(*id);
                    }
                }
            }
        }

        for (id, list) in successors.into_iter().enumerate() {
            self.blocks[id].successors = list;
        }

        let mut predecessors: Vec<Vec<usize>> = vec![Vec::new(); self.blocks.len()];
        for block in &self.blocks {
            for successor in &block.successors {
                predecessors[*successor].push(block.id);
            }
        }

        for (id, list) in predecessors.into_iter().enumerate() {
            self.blocks[id].predecessors = list;
        }
    }

    pub fn entry_block(&self) -> Option<usize> {
        self.by_address.get(&self.entry).copied()
    }

    pub fn block(&self, id: usize) -> Option<&Block> {
        self.blocks.get(id)
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn reverse_postorder(&self) -> Vec<usize> {
        let Some(entry) = self.entry_block() else {
            return Vec::new();
        };

        let mut seen = vec![false; self.blocks.len()];
        let mut order = Vec::new();
        let mut stack = vec![(entry, 0usize)];
        seen[entry] = true;

        while let Some((id, index)) = stack.pop() {
            let successors = &self.blocks[id].successors;
            if index < successors.len() {
                stack.push((id, index + 1));
                let next = successors[index];
                if !seen[next] {
                    seen[next] = true;
                    stack.push((next, 0));
                }
            } else {
                order.push(id);
            }
        }

        order.reverse();
        order
    }

    pub fn dominators(&self) -> Vec<Option<usize>> {
        let Some(entry) = self.entry_block() else {
            return Vec::new();
        };

        let order = self.reverse_postorder();
        let mut position = vec![usize::MAX; self.blocks.len()];
        for (index, id) in order.iter().enumerate() {
            position[*id] = index;
        }

        let mut idom: Vec<Option<usize>> = vec![None; self.blocks.len()];
        idom[entry] = Some(entry);

        let mut changed = true;
        while changed {
            changed = false;

            for id in &order {
                if *id == entry {
                    continue;
                }

                let mut new_idom: Option<usize> = None;

                for predecessor in &self.blocks[*id].predecessors {
                    if idom[*predecessor].is_none() {
                        continue;
                    }
                    new_idom = Some(match new_idom {
                        None => *predecessor,
                        Some(current) => intersect(&idom, &position, *predecessor, current),
                    });
                }

                if new_idom.is_some() && idom[*id] != new_idom {
                    idom[*id] = new_idom;
                    changed = true;
                }
            }
        }

        idom
    }

    pub fn dominates(&self, idom: &[Option<usize>], ancestor: usize, node: usize) -> bool {
        let mut cursor = node;
        loop {
            if cursor == ancestor {
                return true;
            }
            match idom.get(cursor).copied().flatten() {
                Some(parent) if parent != cursor => cursor = parent,
                _ => return false,
            }
        }
    }

    pub fn back_edges(&self, idom: &[Option<usize>]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();

        for block in &self.blocks {
            for successor in &block.successors {
                if self.dominates(idom, *successor, block.id) {
                    out.push((block.id, *successor));
                }
            }
        }

        out
    }

    pub fn natural_loop(&self, tail: usize, header: usize) -> BTreeSet<usize> {
        let mut body = BTreeSet::new();
        body.insert(header);

        let mut stack = vec![tail];
        while let Some(id) = stack.pop() {
            if !body.insert(id) {
                continue;
            }
            for predecessor in &self.blocks[id].predecessors {
                stack.push(*predecessor);
            }
        }

        body
    }

    pub fn loops(&self) -> Vec<LoopInfo> {
        let idom = self.dominators();
        let mut out: BTreeMap<usize, LoopInfo> = BTreeMap::new();

        for (tail, header) in self.back_edges(&idom) {
            let body = self.natural_loop(tail, header);
            let entry = out.entry(header).or_insert_with(|| LoopInfo {
                header,
                tails: Vec::new(),
                body: BTreeSet::new(),
            });
            entry.tails.push(tail);
            entry.body.extend(body);
        }

        out.into_values().collect()
    }

    pub fn post_dominators(&self) -> Vec<Option<usize>> {
        let exits: Vec<usize> = self
            .blocks
            .iter()
            .filter(|block| block.successors.is_empty())
            .map(|block| block.id)
            .collect();

        if exits.is_empty() {
            return vec![None; self.blocks.len()];
        }

        let mut order = self.reverse_postorder();
        order.reverse();

        let mut position = vec![usize::MAX; self.blocks.len()];
        for (index, id) in order.iter().enumerate() {
            position[*id] = index;
        }

        let mut ipdom: Vec<Option<usize>> = vec![None; self.blocks.len()];
        for exit in &exits {
            ipdom[*exit] = Some(*exit);
        }

        let mut changed = true;
        while changed {
            changed = false;

            for id in &order {
                if exits.contains(id) {
                    continue;
                }

                let mut candidate: Option<usize> = None;

                for successor in &self.blocks[*id].successors {
                    if ipdom[*successor].is_none() {
                        continue;
                    }
                    candidate = Some(match candidate {
                        None => *successor,
                        Some(current) => intersect(&ipdom, &position, *successor, current),
                    });
                }

                if candidate.is_some() && ipdom[*id] != candidate {
                    ipdom[*id] = candidate;
                    changed = true;
                }
            }
        }

        ipdom
    }

    pub fn is_reducible(&self) -> bool {
        let idom = self.dominators();

        for block in &self.blocks {
            for successor in &block.successors {
                let is_back = self.dominates(&idom, *successor, block.id);
                let forward_reachable = self.dominates(&idom, block.id, *successor);
                if !is_back && !forward_reachable && self.reaches(*successor, block.id) {
                    return false;
                }
            }
        }

        true
    }

    fn reaches(&self, from: usize, to: usize) -> bool {
        let mut seen = vec![false; self.blocks.len()];
        let mut queue = VecDeque::new();
        queue.push_back(from);

        while let Some(id) = queue.pop_front() {
            if id == to {
                return true;
            }
            if seen[id] {
                continue;
            }
            seen[id] = true;
            for successor in &self.blocks[id].successors {
                queue.push_back(*successor);
            }
        }

        false
    }
}

fn intersect(
    idom: &[Option<usize>],
    position: &[usize],
    mut left: usize,
    mut right: usize,
) -> usize {
    let mut guard = 0usize;

    while left != right {
        guard += 1;
        if guard > 100_000 {
            return left;
        }

        while position
            .get(left)
            .copied()
            .unwrap_or(usize::MAX)
            > position.get(right).copied().unwrap_or(usize::MAX)
        {
            match idom.get(left).copied().flatten() {
                Some(parent) if parent != left => left = parent,
                _ => return right,
            }
        }

        while position
            .get(right)
            .copied()
            .unwrap_or(usize::MAX)
            > position.get(left).copied().unwrap_or(usize::MAX)
        {
            match idom.get(right).copied().flatten() {
                Some(parent) if parent != right => right = parent,
                _ => return left,
            }
        }
    }

    left
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopInfo {
    pub header: usize,
    pub tails: Vec<usize>,
    pub body: BTreeSet<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Instruction, JumpTarget, OpKind};

    fn loop_program() -> VmProgram {
        let mut init = Instruction::new(0, 1, 1);
        init.kind = OpKind::LoadConst;
        init.writes = vec![0];

        let mut test = Instruction::new(1, 2, 2);
        test.kind = OpKind::Branch;
        test.conditional = true;
        test.jumps = vec![JumpTarget { pc: 4, taken_when: Some(false) }];

        let mut body = Instruction::new(2, 3, 3);
        body.kind = OpKind::Binary { operator: "+".into() };
        body.writes = vec![0];

        let mut back = Instruction::new(3, 4, 4);
        back.kind = OpKind::Jump;
        back.terminal = true;
        back.jumps = vec![JumpTarget { pc: 1, taken_when: None }];

        let mut done = Instruction::new(4, 5, 5);
        done.kind = OpKind::Return;
        done.terminal = true;

        VmProgram::from_instructions(vec![init, test, body, back, done])
    }

    #[test]
    fn splits_into_blocks() {
        let program = loop_program();
        let cfg = Cfg::build(&program, 0);
        assert!(cfg.len() >= 3, "{:?}", cfg.blocks);
        assert_eq!(cfg.entry_block(), Some(0));
    }

    #[test]
    fn finds_the_loop() {
        let program = loop_program();
        let cfg = Cfg::build(&program, 0);
        let loops = cfg.loops();
        assert_eq!(loops.len(), 1);
        assert!(loops[0].body.len() >= 2);
        assert!(cfg.is_reducible());
    }

    #[test]
    fn computes_dominators() {
        let program = loop_program();
        let cfg = Cfg::build(&program, 0);
        let idom = cfg.dominators();
        let entry = cfg.entry_block().unwrap();
        for block in &cfg.blocks {
            assert!(cfg.dominates(&idom, entry, block.id), "entry should dominate {}", block.id);
        }
    }
}
