pub mod eval;
pub mod naming;
pub mod passes;
pub mod pipeline;
pub mod splice;
pub mod surface;

pub use eval::{Const, eval, is_pure};
pub use naming::{Evidence, EvidenceIndex, is_junk_name, slug};
pub use passes::{REGISTRY, find, names, pipeline_named, standard_pipeline};
pub use pipeline::{
    Config, MemberReadSpec, Outcome, PassContext, PassSpec, Pipeline, RenameConfig, SourceKind,
    SweepStats, parse_errors, parse_to_string,
};
pub use splice::{Edit, EditLog, find_all};
pub use surface::{RoleMap, SignatureRule, SurfaceIndex, detect_roles};

use wre_core::error::Result;

pub fn deobfuscate(source: &str, config: Config) -> Result<Outcome> {
    standard_pipeline().run(source, config)
}

pub fn beautify(source: &str) -> Result<String> {
    parse_to_string(source, SourceKind::Script)
}
