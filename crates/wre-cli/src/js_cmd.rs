use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;

use wre_core::error::{Error, Result};
use wre_js::pipeline::{Config, RenameConfig, SourceKind};
use wre_js::surface::SurfaceIndex;
use wre_live::mount::{self, MountPlan};
use wre_live::realm::RealmOptions;
use wre_report::table::Table;

use crate::{Context, read_text, target_cmd, write_text};

pub struct DeobfArgs {
    pub input: PathBuf,
    pub target: Option<String>,
    pub out: Option<PathBuf>,
    pub rename: bool,
    pub no_infer: bool,
    pub remove_unused: bool,
    pub only: Vec<String>,
    pub skip: Vec<String>,
    pub sweeps: usize,
    pub stats: bool,
}

pub fn deobf(context: &Context, args: DeobfArgs) -> Result<()> {
    let source = read_text(&args.input)?;

    let manifest = match &args.target {
        Some(name) => Some(target_cmd::load(context, name)?),
        None => None,
    };

    let mut config = match &manifest {
        Some(manifest) => manifest.deobfuscate.to_config(),
        None => Config::structural(),
    };

    config.max_sweeps = args.sweeps.max(1);

    if args.rename {
        config.rename = RenameConfig {
            enabled: true,
            infer: !args.no_infer,
            ..config.rename.clone()
        };
    } else if args.no_infer {
        config.rename.infer = false;
    }

    if args.remove_unused {
        config.remove_unused = true;
    }

    let mut pipeline = match &manifest {
        Some(manifest) => manifest.deobfuscate.pipeline(),
        None => wre_js::standard_pipeline(),
    };

    if !args.only.is_empty() {
        let names: Vec<&str> = args.only.iter().map(String::as_str).collect();
        pipeline = pipeline.only(&names);
    }

    if !args.skip.is_empty() {
        let names: Vec<&str> = args.skip.iter().map(String::as_str).collect();
        pipeline = pipeline.without(&names);
    }

    let outcome = pipeline.run(&source, config)?;

    let destination = args.out.clone().unwrap_or_else(|| {
        let stem = args
            .input
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("script");
        args.input.with_file_name(format!("{stem}.clean.js"))
    });

    write_text(&destination, &outcome.code)?;

    let changes = outcome.changes_by_pass();

    if args.stats && !context.json {
        let mut table = Table::new(&["pass", "changes"]);
        for (name, count) in &changes {
            if *count > 0 {
                table.push(vec![name.clone(), count.to_string()]);
            }
        }
        println!("{}", table.render());
    }

    let record = json!({
        "input": args.input.display().to_string(),
        "output": destination.display().to_string(),
        "bytesBefore": source.len(),
        "bytesAfter": outcome.code.len(),
        "sweeps": outcome.sweeps.len(),
        "converged": outcome.converged,
        "changes": changes,
        "totalChanges": outcome.total_changes(),
    });

    let plain = format!(
        "wrote {}\n  {} bytes -> {} bytes in {} sweeps ({}), {} rewrites\n",
        destination.display(),
        source.len(),
        outcome.code.len(),
        outcome.sweeps.len(),
        if outcome.converged { "converged" } else { "hit the sweep limit" },
        outcome.total_changes()
    );

    context.emit(&record, &plain);
    Ok(())
}

pub fn beautify(context: &Context, input: &std::path::Path, out: Option<PathBuf>) -> Result<()> {
    let source = read_text(input)?;
    let formatted = wre_js::beautify(&source)?;

    match out {
        Some(path) => {
            write_text(&path, &formatted)?;
            context.emit(
                &json!({ "output": path.display().to_string(), "bytes": formatted.len() }),
                &format!("wrote {}\n", path.display()),
            );
        }
        None => print!("{formatted}"),
    }

    Ok(())
}

pub fn passes(context: &Context) -> Result<()> {
    let mut table = Table::new(&["pass", "scope", "what it does"]);
    let mut records = Vec::new();

    for pass in wre_js::REGISTRY {
        table.push(vec![
            pass.name.to_string(),
            if pass.needs_scope { "yes".into() } else { String::new() },
            pass.description.to_string(),
        ]);
        records.push(json!({
            "name": pass.name,
            "needsScope": pass.needs_scope,
            "description": pass.description,
        }));
    }

    context.emit(&json!(records), &table.render());
    Ok(())
}

pub fn surface(
    context: &Context,
    input: &std::path::Path,
    function: Option<String>,
    limit: usize,
) -> Result<()> {
    let source = read_text(input)?;
    let index = SurfaceIndex::build(&source, SourceKind::Script)?;

    if let Some(name) = function {
        let Some(entry) = index.functions.get(&name) else {
            return Err(Error::msg(format!("no function named {name}")));
        };

        let record = json!({
            "name": entry.name,
            "memberPaths": entry.member_paths,
            "globals": entry.globals,
            "calls": entry.calls,
            "statements": entry.statements,
            "branches": entry.branches,
            "loops": entry.loops,
            "signature": index.signature(&name),
            "reachable": index.reachable_from(&name),
        });

        let plain = format!(
            "{}\n  {} statements, {} branches, {} loops\n  surface: {}\n  globals: {}\n  rarest: {}\n",
            entry.name,
            entry.statements,
            entry.branches,
            entry.loops,
            entry.member_paths.iter().cloned().collect::<Vec<_>>().join(", "),
            entry.globals.iter().cloned().collect::<Vec<_>>().join(", "),
            index.signature(&name).unwrap_or_else(|| "none".to_string())
        );

        context.emit(&record, &plain);
        return Ok(());
    }

    let mut entries: Vec<_> = index.functions.values().collect();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.member_paths.len()));

    let mut table = Table::new(&["function", "surface", "calls", "statements", "rarest"]);
    for entry in entries.iter().take(limit) {
        table.push(vec![
            entry.name.clone(),
            entry.member_paths.len().to_string(),
            entry.calls.len().to_string(),
            entry.statements.to_string(),
            index.signature(&entry.name).unwrap_or_default(),
        ]);
    }

    context.emit(
        &json!({ "functions": index.functions.len(), "frequency": index.frequency.len() }),
        &table.render(),
    );

    Ok(())
}

pub fn mount(
    context: &Context,
    input: &std::path::Path,
    target: Option<String>,
    role: Option<String>,
    args: &str,
    eval: Option<String>,
) -> Result<()> {
    let source = read_text(input)?;

    let manifest = match &target {
        Some(name) => Some(target_cmd::load(context, name)?),
        None => None,
    };

    let plan = match &manifest {
        Some(manifest) => manifest.live.to_plan(manifest.deobfuscate.source_kind),
        None => MountPlan::default(),
    };

    if plan.signatures.is_empty() && plan.exports.is_empty() && role.is_some() {
        return Err(Error::msg(
            "the manifest declares no signatures or exports, so no role can be captured",
        ));
    }

    let options = RealmOptions {
        timeout: Duration::from_secs(30),
        clock_ms: manifest.as_ref().and_then(|manifest| manifest.live.clock_ms),
        random_seed: manifest.as_ref().and_then(|manifest| manifest.live.random_seed),
        ..RealmOptions::default()
    };

    let mut mounted = mount::mount(&source, &plan, options)?;

    if let Some(role) = role {
        let parsed: serde_json::Value = serde_json::from_str(args)
            .map_err(|error| Error::msg(format!("--args is not json: {error}")))?;

        let list = parsed
            .as_array()
            .cloned()
            .unwrap_or_else(|| vec![parsed.clone()]);

        let value = mounted.call(&role, &list)?;
        context.emit(&value, &serde_json::to_string_pretty(&value).unwrap_or_default());
        return Ok(());
    }

    if let Some(expression) = eval {
        let value = mounted.realm.eval_json(&expression)?;
        context.emit(&value, &serde_json::to_string_pretty(&value).unwrap_or_default());
        return Ok(());
    }

    let mut table = Table::new(&["role", "captured"]);
    for (role, ok) in &mounted.report.roles {
        table.push(vec![
            role.clone(),
            if *ok { "yes".into() } else { "no".into() },
        ]);
    }

    let record = json!({
        "bytes": mounted.report.bytes,
        "patched": mounted.report.patched,
        "roles": mounted.report.roles,
        "console": mounted.report.records.console.len(),
        "errors": mounted.report.records.errors.len(),
    });

    let plain = format!(
        "mounted {} bytes, {} patches applied\n{}",
        mounted.report.bytes,
        mounted.report.patched,
        table.render()
    );

    context.emit(&record, &plain);
    Ok(())
}
