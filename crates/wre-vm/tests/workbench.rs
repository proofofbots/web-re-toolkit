use std::time::Duration;

use serde_json::json;

use wre_js::pipeline::SourceKind;
use wre_live::realm::{Realm, RealmOptions};
use wre_vm::discover::discover;
use wre_vm::ir::{Instruction, JumpTarget, OpKind, Operand, VmProgram};
use wre_vm::lift::{LiftOptions, lift};
use wre_vm::probe::{FrameModel, Prober};
use wre_vm::trace::{TraceEntry, align, coverage, handler_identity, label_program};

const TOY_VM: &str = r#"
var CODE = [0, 5, 1, 2, 3, 3, 9];

var HANDLERS = [
  function loadConst(state, read, store) { store(0, read()); },
  function add(state, read, store) { var a = read(); var b = read(); store(2, a + b); },
  function branchIfFalsy(state, read, store) { if (!read()) { state.pc = read(); } },
  function halt(state, read, store) { state.done = true; state.result = read(); }
];

function run(state, read, store) {
  while (!state.done) {
    var opcode = CODE[state.pc++];
    HANDLERS[opcode](state, read, store);
  }
  return state.result;
}
"#;

const FRAME_MODEL: &str = r#"
function (recorder, options) {
  var state = new Proxy({}, {
    get: function (target, key) {
      if (key === "pc") return 0;
      if (key === "done") return false;
      return recorder.sentinel("state." + String(key));
    },
    set: function (target, key, value) {
      if (key === "pc") { recorder.jump(value); } else { recorder.write(String(key)); }
      return true;
    }
  });

  var read = function () { return recorder.read(); };
  var store = function (slot) { recorder.write(slot); };

  return [state, read, store];
}
"#;

fn prober() -> Prober {
    let mut realm = Realm::new(RealmOptions {
        timeout: Duration::from_secs(5),
        ..RealmOptions::default()
    })
    .expect("realm");

    realm.eval_unit(TOY_VM, "toy-vm").expect("toy vm loaded");

    let mut prober = Prober::from_realm(realm).expect("kernel installed");
    prober
        .install(FrameModel::new(FRAME_MODEL))
        .expect("frame model installed");
    prober
}

#[test]
fn discovers_the_dispatch_loop_and_table() {
    let report = discover(TOY_VM, SourceKind::Script).unwrap();
    let best = report.best_dispatch().expect("dispatch candidate");
    assert_eq!(best.callee, "HANDLERS[...]");
    assert_eq!(best.arity, 3);
    assert!(best.all_identifier_arguments);

    let table = report.largest_table().expect("table candidate");
    assert_eq!(table.name.as_deref(), Some("HANDLERS"));
    assert_eq!(table.length, 4);
    assert_eq!(table.uniform_arity, Some(3));
}

#[test]
fn probes_operand_and_register_shape() {
    let mut prober = prober();

    let load = prober.profile("HANDLERS[0]", 0).unwrap();
    assert_eq!(load.reads, 1);
    assert_eq!(load.writes, 1);
    assert_eq!(load.jumps, 0);
    assert!(!load.conditional);
    assert_eq!(load.straight.slots_written(), vec![0]);

    let add = prober.profile("HANDLERS[1]", 1).unwrap();
    assert_eq!(add.reads, 2);
    assert_eq!(add.writes, 1);
    assert_eq!(add.straight.slots_written(), vec![2]);
    assert_eq!(add.kind, OpKind::Binary { operator: "+".to_string() });
}

#[test]
fn detects_a_conditional_branch_by_differential_probing() {
    let mut prober = prober();

    let branch = prober.profile("HANDLERS[2]", 2).unwrap();
    assert!(branch.conditional, "{branch:?}");
    assert_eq!(branch.jumps, 0, "the straight run must not jump");

    let alternate = branch.alternate.expect("alternate run");
    assert_eq!(alternate.jumps.len(), 1, "the falsy run must jump");
    assert_eq!(alternate.reads.len(), 2);
}

#[test]
fn profiles_a_whole_table() {
    let mut prober = prober();
    let profiles = prober.profile_table("HANDLERS", 0).unwrap();

    assert_eq!(profiles.len(), 4);
    assert!(profiles.iter().any(|profile| profile.conditional));
    assert!(profiles.iter().all(|profile| !profile.straight.failed()));
}

#[test]
fn handler_sources_survive_the_boundary() {
    let mut prober = prober();
    let sources = prober.handler_sources("HANDLERS", 4).unwrap();

    assert_eq!(sources.len(), 4);
    assert!(sources[0].contains("loadConst"));
    assert!(sources[2].contains("state.pc"));
}

fn decoded_program() -> VmProgram {
    let mut load = Instruction::new(0, 0, 2);
    load.kind = OpKind::LoadConst;
    load.writes = vec![0];
    load.operands = vec![Operand::Int { value: 5 }];
    load.label = Some("loadConst".to_string());

    let mut second = Instruction::new(2, 0, 4);
    second.kind = OpKind::LoadConst;
    second.writes = vec![1];
    second.operands = vec![Operand::Int { value: 7 }];
    second.label = Some("loadConst".to_string());

    let mut add = Instruction::new(4, 1, 5);
    add.kind = OpKind::Binary { operator: "+".to_string() };
    add.writes = vec![2];
    add.operands = vec![Operand::Register { index: 0 }, Operand::Register { index: 1 }];
    add.label = Some("add".to_string());

    let mut branch = Instruction::new(5, 2, 7);
    branch.kind = OpKind::Branch;
    branch.conditional = true;
    branch.operands = vec![Operand::Register { index: 2 }];
    branch.jumps = vec![JumpTarget { pc: 8, taken_when: Some(false) }];
    branch.label = Some("branchIfFalsy".to_string());

    let mut fallback = Instruction::new(7, 0, 8);
    fallback.kind = OpKind::LoadConst;
    fallback.writes = vec![2];
    fallback.operands = vec![Operand::Int { value: 1 }];

    let mut halt = Instruction::new(8, 3, 9);
    halt.kind = OpKind::Return;
    halt.terminal = true;
    halt.operands = vec![Operand::Register { index: 2 }];
    halt.label = Some("halt".to_string());

    VmProgram::from_instructions(vec![load, second, add, branch, fallback, halt])
}

#[test]
fn lifts_the_decoded_program_to_readable_javascript() {
    let program = decoded_program();
    program.validate().unwrap();

    let (code, report) = lift(&program, &[0], LiftOptions::default());

    assert!(code.contains("r0 = 5;"), "{code}");
    assert!(code.contains("r2 = r0 + r1;"), "{code}");
    assert!(code.contains("if (r2)"), "{code}");
    assert!(code.contains("return r2;"), "{code}");
    assert_eq!(report.structured, 1);
    assert!(report.unknown_opcodes.is_empty());
}

#[test]
fn a_listing_marks_jump_targets() {
    let listing = decoded_program().listing();
    assert!(listing.contains("@8:"), "{listing}");
    assert!(listing.contains("branchIfFalsy"), "{listing}");
}

#[test]
fn opcode_labels_come_from_a_trace() {
    let mut prober = prober();
    let sources = prober.handler_sources("HANDLERS", 4).unwrap();

    let trace: Vec<TraceEntry> = vec![(0usize, 0u32), (2, 0), (4, 1), (5, 2), (8, 3)]
        .into_iter()
        .enumerate()
        .map(|(sequence, (pc, opcode))| TraceEntry {
            pc,
            opcode,
            handler: Some(handler_identity(&sources[opcode as usize])),
            frame: None,
            sequence,
        })
        .collect();

    let map = align(&trace);
    assert_eq!(map.coverage(), 4);
    assert!(map.conflicts.is_empty());

    let mut labels = std::collections::BTreeMap::new();
    for (opcode, _) in &map.by_opcode {
        labels.insert(*opcode, format!("h{opcode}"));
    }

    let mut program = decoded_program();
    let applied = label_program(&mut program, &labels);
    assert_eq!(applied, program.len());

    let stats = coverage(&program, &trace);
    assert_eq!(stats.visited.len(), 5);
    assert_eq!(stats.unreached, vec![7]);
    assert!(stats.ratio() > 0.8);
}

#[test]
fn the_toy_vm_actually_runs_in_the_realm() {
    let mut realm = Realm::new(RealmOptions::default()).unwrap();
    realm.eval_unit(TOY_VM, "toy-vm").unwrap();

    let value = realm
        .eval(
            r#"
            (function () {
                var state = { pc: 0, done: false, result: null };
                var regs = [];
                var read = function () { return CODE[state.pc++]; };
                var store = function (slot, value) { regs[slot] = value; };
                var guard = 0;
                while (!state.done && guard++ < 32) {
                    var opcode = CODE[state.pc++];
                    HANDLERS[opcode](state, read, store);
                }
                return { regs: regs, result: state.result };
            })()
            "#,
            "toy-run",
        )
        .unwrap();

    assert_eq!(value, json!({ "regs": [5, null, 5], "result": 9 }));
}
