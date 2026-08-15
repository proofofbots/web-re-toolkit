use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use wre_core::error::{Error, Result};
use wre_ident::drift::{Lock, State, compare as compare_builds};
use wre_ident::locate::{Locator, Resolution};
use wre_ident::shape::ShapeIndex;
use wre_js::equivalence;
use wre_js::integrity::Guard;
use wre_js::pipeline::SourceKind;
use wre_oracle::fidelity;
use wre_signals::vector;

use crate::{Context, read_json, read_text, target_cmd};

fn json_of<T: serde::Serialize>(value: &T) -> Result<Value> {
    serde_json::to_value(value)
        .map_err(|error| Error::msg(format!("could not render the report: {error}")))
}

fn json_text<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value)
        .map_err(|error| Error::msg(format!("could not render the report: {error}")))
}

fn index_of(path: &Path, kind: SourceKind) -> Result<ShapeIndex> {
    let source = read_text(path)?;
    ShapeIndex::build(&source, kind)
}

fn kind_of(module: bool) -> SourceKind {
    if module { SourceKind::Module } else { SourceKind::Script }
}

pub fn locate(
    context: &Context,
    input: &Path,
    target: &str,
    module: bool,
    lock_to: Option<PathBuf>,
) -> Result<()> {
    let manifest = target_cmd::load(context, target)?;

    if manifest.locate.is_empty() {
        return Err(Error::msg(format!(
            "target {target} declares no [[locate]] rules, so there is nothing to look for"
        )));
    }

    let source = read_text(input)?;
    let index = ShapeIndex::build(&source, kind_of(module))?;
    let resolution = Locator::new(&index).resolve(&manifest.locate)?;

    let plain = render_resolution(&resolution);
    context.emit(&json_of(&resolution)?, &plain);

    if let Some(path) = lock_to {
        let digest = wre_core::digest::sha256_short(source.as_bytes());
        let lock = Lock::from_resolution(digest, &index, &resolution);

        std::fs::write(&path, json_text(&lock)?)
            .map_err(wre_core::error::io(&path))?;

        context.note(&format!(
            "wrote {} roles to {}",
            lock.roles.len(),
            path.display()
        ));
    }

    Ok(())
}

fn render_resolution(resolution: &Resolution) -> String {
    let mut out = String::from("| role | binding | score | evidence |\n| --- | --- | --- | --- |\n");

    for (role, candidate) in &resolution.roles {
        out.push_str(&format!(
            "| {role} | {} | {:.2} | {} |\n",
            candidate.name,
            candidate.score,
            candidate.matched.join("; ")
        ));
    }

    if !resolution.ambiguous.is_empty() {
        out.push_str(&format!(
            "\nambiguous, two candidates scored within the margin: {}\n",
            resolution.ambiguous.join(", ")
        ));
    }

    if !resolution.unresolved.is_empty() {
        out.push_str(&format!("\nnot found: {}\n", resolution.unresolved.join(", ")));
    }

    out
}

pub fn drift(context: &Context, lock_path: &Path, input: &Path, module: bool) -> Result<()> {
    let text = read_text(lock_path)?;
    let lock: Lock = serde_json::from_str(&text).map_err(|error| {
        Error::msg(format!(
            "{} is not a lock this build can read ({error}), re-run `wre locate --lock`",
            lock_path.display()
        ))
    })?;

    lock.readable()?;

    let source = read_text(input)?;
    let digest = wre_core::digest::sha256_short(source.as_bytes());

    if lock.is_current(&digest) {
        context.emit(
            &json!({ "digest": digest, "drift": [] }),
            "the script is byte for byte the one this lock was written against\n",
        );
        return Ok(());
    }

    let index = ShapeIndex::build(&source, kind_of(module))?;
    let report = lock.check(&index, 0.5);

    let mut plain = String::new();
    for entry in &report {
        plain.push_str(&entry.describe());
        plain.push('\n');
    }

    let review = report.iter().filter(|entry| entry.needs_review()).count();
    plain.push_str(&format!("\n{review} of {} roles moved\n", report.len()));

    context.emit(&json!({ "digest": digest, "drift": report }), &plain);

    if report.iter().any(|entry| matches!(entry.state, State::Lost)) {
        return Err(Error::msg(
            "at least one role is no longer findable, re-run `wre locate` to rebuild the lock",
        ));
    }

    Ok(())
}

pub fn builds(
    context: &Context,
    before: &Path,
    after: &Path,
    module: bool,
    threshold: f64,
) -> Result<()> {
    let left = index_of(before, kind_of(module))?;
    let right = index_of(after, kind_of(module))?;

    let diff = compare_builds(&left, &right, threshold);

    let mut plain = String::from("| before | after | verdict | shared |\n| --- | --- | --- | --- |\n");
    for pair in &diff.pairs {
        if pair.verdict == wre_ident::drift::Verdict::Identical {
            continue;
        }
        plain.push_str(&format!(
            "| {} | {} | {:?} | {:.1}% |\n",
            pair.before,
            pair.after,
            pair.verdict,
            pair.similarity * 100.0
        ));
    }

    if !diff.gone.is_empty() {
        plain.push_str(&format!("\ngone: {}\n", diff.gone.join(", ")));
    }
    if !diff.added.is_empty() {
        plain.push_str(&format!("new: {}\n", diff.added.join(", ")));
    }
    plain.push_str(&format!("\n{}\n", diff.summary()));

    context.emit(&json_of(&diff)?, &plain);
    Ok(())
}

pub fn integrity(
    context: &Context,
    input: &Path,
    target: &str,
    resign: bool,
    out: Option<PathBuf>,
) -> Result<()> {
    let manifest = target_cmd::load(context, target)?;

    let guard: Guard = manifest.integrity.ok_or_else(|| {
        Error::msg(format!("target {target} declares no [integrity] guard"))
    })?;

    let source = read_text(input)?;

    if !resign {
        let report = guard.verify(&source)?;
        context.emit(&json_of(&report)?, &format!("{}\n", report.describe()));

        if !report.holds() {
            return Err(Error::msg("the script does not match its own guard"));
        }
        return Ok(());
    }

    let (signed, report) = guard.resign(&source)?;
    let destination = out.unwrap_or_else(|| input.to_path_buf());

    std::fs::write(&destination, &signed).map_err(wre_core::error::io(&destination))?;

    context.emit(
        &json!({ "stored": report.stored, "written": report.computed, "path": destination }),
        &format!(
            "re-signed {} from {} to {}\n",
            destination.display(),
            report.stored,
            report.computed
        ),
    );

    Ok(())
}

pub fn equivalent(context: &Context, original: &Path, rewritten: &Path, module: bool) -> Result<()> {
    let before = read_text(original)?;
    let after = read_text(rewritten)?;

    let found = equivalence::compare(&before, &after, kind_of(module))?;
    context.emit(&json_of(&found)?, &format!("{}\n", found.describe()));

    if !found.holds() {
        return Err(Error::msg("the rewrite is not equivalent to the original"));
    }

    Ok(())
}

pub fn grade(context: &Context, real: &[PathBuf], built: &Path) -> Result<()> {
    if real.len() < 2 {
        return Err(Error::msg(
            "pass at least two real payloads, otherwise a volatile field looks like a defect",
        ));
    }

    let runs: Vec<Value> = real
        .iter()
        .map(|path| read_json(path))
        .collect::<Result<Vec<Value>>>()?;

    let made = read_json(built)?;
    let graded = fidelity::compare(&runs, &made)?;

    let plain = format!("{}\n\n{}\n", graded.summary(), graded.render());
    context.emit(&json_of(&graded)?, &plain);

    if !graded.passes() {
        return Err(Error::msg(format!(
            "{} fields do not hold up",
            graded.failures().len()
        )));
    }

    Ok(())
}

pub fn align(context: &Context, before: &[PathBuf], after: &[PathBuf]) -> Result<()> {
    let load = |paths: &[PathBuf]| -> Result<Vec<Vec<Value>>> {
        paths
            .iter()
            .map(|path| {
                let value = read_json(path)?;
                value.as_array().cloned().ok_or_else(|| {
                    Error::msg(format!("{} is not an array of slots", path.display()))
                })
            })
            .collect()
    };

    let left = load(before)?;
    let right = load(after)?;

    let (alignment, noisy) = vector::stable_align(&left, &right)?;
    let width = left.first().map(Vec::len).unwrap_or(0);

    let mut plain = format!(
        "{} of {width} slots aligned across {} runs\n",
        alignment.pairs.len(),
        left.len()
    );

    if let Some((shift, agreement)) = alignment.dominant_shift() {
        plain.push_str(&format!(
            "most slots moved by {shift}, {:.0}% agree\n",
            agreement * 100.0
        ));
    }

    if !noisy.is_empty() {
        plain.push_str(&format!("{} slots were noisy and left out\n", noisy.len()));
    }
    if !alignment.ambiguous.is_empty() {
        plain.push_str(&format!(
            "{} slots carry a value that appears more than once, so they stayed ambiguous\n",
            alignment.ambiguous.len()
        ));
    }

    plain.push_str("\n| before | after |\n| --- | --- |\n");
    for (slot, target) in &alignment.pairs {
        plain.push_str(&format!("| {slot} | {target} |\n"));
    }

    context.emit(&json_of(&alignment)?, &plain);
    Ok(())
}
