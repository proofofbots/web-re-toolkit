use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;

use wre_core::error::{Error, Result};
use wre_js::pipeline::SourceKind;
use wre_live::realm::{Realm, RealmOptions};
use wre_report::table::Table;
use wre_vm::cfg::Cfg;
use wre_vm::discover::discover;
use wre_vm::ir::VmProgram;
use wre_vm::lift::{LiftMode, LiftOptions, lift};
use wre_vm::probe::{FrameModel, Prober};
use wre_vm::trace::{TraceEntry, align, from_sources, permutation};

use crate::args::VmCommand;
use crate::{Context, read_json, read_text, write_text};

pub fn run(context: &Context, command: VmCommand) -> Result<()> {
    match command {
        VmCommand::Discover { input } => discover_cmd(context, &input),
        VmCommand::Probe { input, table, frame, limit, out } => {
            probe_cmd(context, &input, &table, &frame, limit, out)
        }
        VmCommand::Listing { program } => listing_cmd(context, &program),
        VmCommand::Lift { program, entry, dispatch, annotate, out } => {
            lift_cmd(context, &program, entry, dispatch, annotate, out)
        }
        VmCommand::Cfg { program, entry } => cfg_cmd(context, &program, entry),
        VmCommand::Align { trace, against } => align_cmd(context, &trace, against),
    }
}

fn discover_cmd(context: &Context, input: &std::path::Path) -> Result<()> {
    let source = read_text(input)?;
    let report = discover(&source, SourceKind::Script)?;

    let mut dispatch = Table::new(&["callee", "arity", "loop", "identifiers", "score"]);
    for candidate in report.dispatch.iter().take(10) {
        dispatch.push(vec![
            candidate.callee.clone(),
            candidate.arity.to_string(),
            candidate.loop_kind.clone(),
            if candidate.all_identifier_arguments { "yes".into() } else { String::new() },
            candidate.score.to_string(),
        ]);
    }

    let mut tables = Table::new(&["name", "entries", "arity", "offset"]);
    for candidate in report.tables.iter().take(10) {
        tables.push(vec![
            candidate.name.clone().unwrap_or_else(|| "<anonymous>".into()),
            candidate.length.to_string(),
            candidate
                .uniform_arity
                .map(|value| value.to_string())
                .unwrap_or_default(),
            candidate.start.to_string(),
        ]);
    }

    let plain = format!(
        "{} loops scanned\n\ndispatch candidates\n{}\nhandler tables\n{}",
        report.loops,
        dispatch.render(),
        tables.render()
    );

    context.emit(&json!(report), &plain);
    Ok(())
}

fn probe_cmd(
    context: &Context,
    input: &std::path::Path,
    table: &str,
    frame: &std::path::Path,
    limit: usize,
    out: Option<PathBuf>,
) -> Result<()> {
    let source = read_text(input)?;
    let model = read_text(frame)?;

    let mut realm = Realm::new(RealmOptions {
        timeout: Duration::from_secs(30),
        ..RealmOptions::default()
    })?;

    realm.eval_unit(&source, "vm:target")?;

    let mut prober = Prober::from_realm(realm)?;
    prober.install(FrameModel::new(model))?;

    let profiles = prober.profile_table(table, limit)?;

    let mut rendered = Table::new(&["opcode", "reads", "writes", "jumps", "conditional", "kind"]);
    for profile in &profiles {
        rendered.push(vec![
            profile.index.to_string(),
            profile.reads.to_string(),
            profile.writes.to_string(),
            profile.jumps.to_string(),
            if profile.conditional { "yes".into() } else { String::new() },
            profile.kind.name(),
        ]);
    }

    if let Some(path) = &out {
        write_text(path, &serde_json::to_string_pretty(&profiles).unwrap_or_default())?;
    }

    let plain = format!(
        "{} handlers probed{}\n{}",
        profiles.len(),
        out.as_ref()
            .map(|path| format!(", written to {}", path.display()))
            .unwrap_or_default(),
        rendered.render()
    );

    context.emit(&json!(profiles), &plain);
    Ok(())
}

fn load_program(path: &std::path::Path) -> Result<VmProgram> {
    let value = read_json(path)?;
    serde_json::from_value(value)
        .map_err(|error| Error::msg(format!("{} is not a vm program: {error}", path.display())))
}

fn listing_cmd(context: &Context, path: &std::path::Path) -> Result<()> {
    let program = load_program(path)?;
    program.validate()?;

    context.emit(
        &json!({
            "instructions": program.len(),
            "entry": program.entry,
            "strings": program.strings().len(),
            "opcodes": program.opcode_histogram(),
        }),
        &program.listing(),
    );

    Ok(())
}

fn lift_cmd(
    context: &Context,
    path: &std::path::Path,
    entries: Vec<usize>,
    dispatch: bool,
    annotate: bool,
    out: Option<PathBuf>,
) -> Result<()> {
    let program = load_program(path)?;
    program.validate()?;

    let entries = if entries.is_empty() {
        let carved = wre_vm::ir::carve_functions(&program);
        if carved.is_empty() {
            vec![program.entry]
        } else {
            carved.into_iter().map(|entry| entry.entry).collect()
        }
    } else {
        entries
    };

    let options = LiftOptions {
        mode: if dispatch { LiftMode::Dispatch } else { LiftMode::Structured },
        annotate,
        ..LiftOptions::default()
    };

    let (code, report) = lift(&program, &entries, options);

    match &out {
        Some(path) => write_text(path, &code)?,
        None => {
            if !context.json {
                print!("{code}");
            }
        }
    }

    let record = json!({
        "functions": report.functions,
        "structured": report.structured,
        "dispatched": report.dispatched,
        "unknownOpcodes": report.unknown_opcodes,
        "bytes": code.len(),
        "output": out.as_ref().map(|path| path.display().to_string()),
    });

    if context.json {
        context.emit(&record, "");
    } else if out.is_some() {
        println!(
            "lifted {} functions ({} structured, {} as a dispatch loop), {} unknown opcodes",
            report.functions,
            report.structured,
            report.dispatched,
            report.unknown_opcodes.len()
        );
    }

    Ok(())
}

fn cfg_cmd(context: &Context, path: &std::path::Path, entry: usize) -> Result<()> {
    let program = load_program(path)?;
    let entry = if entry == 0 { program.entry } else { entry };
    let cfg = Cfg::build(&program, entry);

    let mut table = Table::new(&["block", "start", "instructions", "successors", "flags"]);
    for block in &cfg.blocks {
        let mut flags = Vec::new();
        if block.conditional {
            flags.push("conditional");
        }
        if block.terminal {
            flags.push("terminal");
        }

        table.push(vec![
            block.id.to_string(),
            block.start.to_string(),
            block.addresses.len().to_string(),
            block
                .successors
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            flags.join(" "),
        ]);
    }

    let loops = cfg.loops();

    let record = json!({
        "entry": entry,
        "blocks": cfg.len(),
        "reducible": cfg.is_reducible(),
        "loops": loops.iter().map(|info| json!({ "header": info.header, "body": info.body.len(), "tails": info.tails })).collect::<Vec<_>>(),
    });

    let plain = format!(
        "{} blocks from @{entry}, {} natural loops, {}\n{}",
        cfg.len(),
        loops.len(),
        if cfg.is_reducible() { "reducible" } else { "irreducible, the lifter will emit a dispatch loop" },
        table.render()
    );

    context.emit(&record, &plain);
    Ok(())
}

fn align_cmd(context: &Context, trace: &std::path::Path, against: Option<PathBuf>) -> Result<()> {
    let value = read_json(trace)?;
    let entries: Vec<TraceEntry> = serde_json::from_value(value)
        .map_err(|error| Error::msg(format!("trace did not parse: {error}")))?;

    let map = align(&entries);

    let mut record = json!({
        "opcodes": map.coverage(),
        "conflicts": map.conflicts,
        "samples": map.samples,
    });

    let mut plain = format!(
        "{} opcodes mapped to handler identities, {} conflicts\n",
        map.coverage(),
        map.conflicts.len()
    );

    if let Some(path) = against {
        let sources: Vec<String> = serde_json::from_value(read_json(&path)?)
            .map_err(|error| Error::msg(format!("{} is not a handler source list: {error}", path.display())))?;

        let other = from_sources(&sources);
        let mapping: BTreeMap<u32, u32> = permutation(&map, &other);

        let mut table = Table::new(&["was", "now"]);
        for (from, to) in &mapping {
            table.push(vec![from.to_string(), to.to_string()]);
        }

        record["permutation"] = json!(mapping);
        plain.push_str(&format!(
            "\n{} opcodes matched across builds\n{}",
            mapping.len(),
            table.render()
        ));
    }

    context.emit(&record, &plain);
    Ok(())
}
