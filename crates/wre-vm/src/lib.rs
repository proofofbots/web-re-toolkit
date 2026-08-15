pub mod cfg;
pub mod discover;
pub mod ir;
pub mod lift;
pub mod probe;
pub mod trace;

pub use cfg::{Block, Cfg, LoopInfo};
pub use discover::{DiscoveryReport, DispatchCandidate, TableCandidate, discover};
pub use ir::{FunctionRange, Instruction, JumpTarget, OpKind, Operand, VmProgram, carve_functions};
pub use lift::{LiftMode, LiftOptions, LiftReport, Lifter, lift};
pub use probe::{FrameModel, HandlerProfile, ProbeRecord, Prober, classify_source};
pub use trace::{Coverage, OpcodeMap, TraceEntry, align, coverage, handler_identity, permutation};
