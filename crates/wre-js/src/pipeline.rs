use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use oxc_allocator::GetAllocator;
use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_ast::builder::AstBuilder;
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_parser::{ParseOptions, Parser};
use oxc_semantic::{Scoping, SemanticBuilder};
use oxc_span::SourceType;
use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

use crate::eval::Const;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberReadSpec {
    pub function: String,
    pub object_arg: usize,
    pub key_arg: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RenameConfig {
    pub enabled: bool,
    pub infer: bool,
    #[serde(default)]
    pub reserved: HashSet<String>,
    #[serde(default)]
    pub forced: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub max_sweeps: usize,
    pub call_values: HashMap<String, Const>,
    pub thunk_values: HashMap<String, Const>,
    pub index_tables: HashMap<String, Vec<Const>>,
    pub member_reads: Vec<MemberReadSpec>,
    pub hash_names: HashMap<u32, String>,
    pub hash_functions: Vec<String>,
    pub rename: RenameConfig,
    pub inline_global_aliases: bool,
    pub aggressive_member_access: bool,
    pub drop_debugger: bool,
    pub remove_unused: bool,
    pub source_type: SourceKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    #[default]
    Script,
    Module,
}

impl SourceKind {
    pub fn to_source_type(self) -> SourceType {
        match self {
            SourceKind::Script => SourceType::cjs(),
            SourceKind::Module => SourceType::mjs(),
        }
    }
}

impl Config {
    pub fn readable() -> Self {
        Self {
            max_sweeps: 8,
            rename: RenameConfig { enabled: true, infer: true, ..RenameConfig::default() },
            inline_global_aliases: true,
            drop_debugger: false,
            remove_unused: true,
            ..Config::default()
        }
    }

    pub fn structural() -> Self {
        Self { max_sweeps: 8, ..Config::default() }
    }
}

pub struct PassContext<'a> {
    pub builder: AstBuilder<'a>,
    pub config: Arc<Config>,
    pub scoping: Option<Scoping>,
    pub notes: Vec<String>,
}

impl<'a> PassContext<'a> {
    pub fn new(allocator: &'a Allocator, config: Arc<Config>) -> Self {
        Self {
            builder: AstBuilder::new(allocator),
            config,
            scoping: None,
            notes: Vec::new(),
        }
    }

    pub fn alloc(&self, text: &str) -> &'a str {
        self.builder.allocator().alloc_str(text)
    }

    pub fn scoping(&self) -> Option<&Scoping> {
        self.scoping.as_ref()
    }

    pub fn note(&mut self, text: impl Into<String>) {
        self.notes.push(text.into());
    }
}

pub type PassFn = for<'a> fn(&mut Program<'a>, &mut PassContext<'a>) -> usize;

#[derive(Clone, Copy)]
pub struct PassSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub needs_scope: bool,
    pub run: PassFn,
}

impl std::fmt::Debug for PassSpec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PassSpec")
            .field("name", &self.name)
            .field("needs_scope", &self.needs_scope)
            .finish()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SweepStats {
    pub sweep: usize,
    pub changes: BTreeMap<String, usize>,
    pub bytes_before: usize,
    pub bytes_after: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    pub code: String,
    pub sweeps: Vec<SweepStats>,
    pub converged: bool,
    pub notes: Vec<String>,
}

impl Outcome {
    pub fn total_changes(&self) -> usize {
        self.sweeps
            .iter()
            .flat_map(|sweep| sweep.changes.values())
            .sum()
    }

    pub fn changes_by_pass(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for sweep in &self.sweeps {
            for (name, count) in &sweep.changes {
                *out.entry(name.clone()).or_insert(0) += count;
            }
        }
        out
    }
}

pub struct Pipeline {
    passes: Vec<PassSpec>,
}

impl Pipeline {
    pub fn new(passes: Vec<PassSpec>) -> Self {
        Self { passes }
    }

    pub fn passes(&self) -> &[PassSpec] {
        &self.passes
    }

    pub fn only(&self, names: &[&str]) -> Pipeline {
        Pipeline::new(
            self.passes
                .iter()
                .copied()
                .filter(|pass| names.contains(&pass.name))
                .collect(),
        )
    }

    pub fn without(&self, names: &[&str]) -> Pipeline {
        Pipeline::new(
            self.passes
                .iter()
                .copied()
                .filter(|pass| !names.contains(&pass.name))
                .collect(),
        )
    }

    pub fn prefix(&self, count: usize) -> Pipeline {
        Pipeline::new(self.passes.iter().copied().take(count).collect())
    }

    pub fn run(&self, source: &str, config: Config) -> Result<Outcome> {
        let max_sweeps = config.max_sweeps.max(1);
        let shared = Arc::new(config);

        let mut current = source.to_string();
        let mut sweeps = Vec::new();
        let mut notes = Vec::new();
        let mut converged = false;

        for index in 0..max_sweeps {
            let bytes_before = current.len();
            let (next, changes, mut sweep_notes) = self.sweep(&current, Arc::clone(&shared))?;

            let stats = SweepStats {
                sweep: index,
                changes,
                bytes_before,
                bytes_after: next.len(),
            };

            let stable = next == current;
            let quiet = stats.changes.values().all(|count| *count == 0);
            notes.append(&mut sweep_notes);
            sweeps.push(stats);
            current = next;

            if stable || quiet {
                converged = true;
                break;
            }
        }

        Ok(Outcome { code: current, sweeps, converged, notes })
    }

    fn sweep(
        &self,
        source: &str,
        config: Arc<Config>,
    ) -> Result<(String, BTreeMap<String, usize>, Vec<String>)> {
        let allocator = Allocator::default();
        let source_type = config.source_type.to_source_type();

        let parsed = Parser::new(&allocator, source, source_type)
            .with_options(ParseOptions {
                preserve_parens: false,
                ..ParseOptions::default()
            })
            .parse();

        if parsed.panicked {
            let first = parsed
                .diagnostics
                .first()
                .map(|error| error.to_string())
                .unwrap_or_else(|| "unknown parse failure".to_string());
            return Err(Error::msg(format!("parse failed: {first}")));
        }

        let mut program = parsed.program;
        let mut context = PassContext::new(&allocator, config);
        let mut changes = BTreeMap::new();

        for pass in &self.passes {
            if pass.needs_scope {
                let scoping: Scoping = SemanticBuilder::new()
                    .build(&program)
                    .semantic
                    .into_scoping();
                context.scoping = Some(scoping);
            } else {
                context.scoping = None;
            }

            let count = (pass.run)(&mut program, &mut context);
            *changes.entry(pass.name.to_string()).or_insert(0) += count;
        }

        let printed = Codegen::new()
            .with_options(CodegenOptions::default())
            .build(&program);

        Ok((printed.code, changes, std::mem::take(&mut context.notes)))
    }
}

pub fn parse_to_string(source: &str, kind: SourceKind) -> Result<String> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, kind.to_source_type())
        .with_options(ParseOptions {
            preserve_parens: false,
            ..ParseOptions::default()
        })
        .parse();

    if parsed.panicked {
        let first = parsed
            .diagnostics
            .first()
            .map(|error| error.to_string())
            .unwrap_or_else(|| "unknown parse failure".to_string());
        return Err(Error::msg(format!("parse failed: {first}")));
    }

    Ok(Codegen::new()
        .with_options(CodegenOptions::default())
        .build(&parsed.program)
        .code)
}

pub fn parse_errors(source: &str, kind: SourceKind) -> Vec<String> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, kind.to_source_type()).parse();
    parsed.diagnostics.iter().map(|error| error.to_string()).collect()
}
