use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use wre_core::address::Address;
use wre_core::bundle::CaptureBundle;
use wre_core::error::{Error, Result};
use wre_net::h2::fingerprint_bytes;
use wre_net::tls::ClientHello;
use wre_report::acceptance::Acceptance;
use wre_report::baseline::{Baseline, diff_maps, render_diff};
use wre_report::table::Table;
use wre_variants::markers::automation_markers;
use wre_variants::sweep::{self as sweep_engine, Knob, SweepOptions, render_arms, render_signal_map};
use wre_wire::codec::{Codec, JsonCodec, verify_roundtrip};

use crate::args::TlsCommand;
use crate::{Context, read_bytes, read_json, read_text, target_cmd};

pub fn tls(context: &Context, command: TlsCommand) -> Result<()> {
    match command {
        TlsCommand::Hello { input, hex } => {
            let bytes = load_bytes(&input, hex)?;

            let hello = ClientHello::parse_record(&bytes)
                .or_else(|_| ClientHello::parse_handshake(&bytes))
                .or_else(|_| ClientHello::parse_body(&bytes))?;

            let summary = hello.summary();

            let plain = format!(
                "ja3  {}\n     {}\nja4  {}\nsni  {}\nalpn {}\n{} ciphers, {} extensions{}\n",
                summary.ja3.hash,
                summary.ja3.text,
                summary.ja4,
                summary.server_name.clone().unwrap_or_else(|| "none".into()),
                if summary.alpn.is_empty() { "none".to_string() } else { summary.alpn.join(", ") },
                summary.cipher_count,
                summary.extension_count,
                if summary.grease_present { ", grease present" } else { "" }
            );

            context.emit(&json!(summary), &plain);
            Ok(())
        }
        TlsCommand::H2 { input, hex } => {
            let bytes = load_bytes(&input, hex)?;
            let fingerprint = fingerprint_bytes(&bytes)?;

            let mut table = Table::new(&["setting", "value"]);
            for (name, value) in fingerprint.describe_settings() {
                table.push(vec![name, value.to_string()]);
            }

            let plain = format!(
                "akamai {}\nsha256 {}\npseudo header order {}\n{}",
                fingerprint.akamai_text,
                fingerprint.akamai_hash,
                fingerprint.pseudo_header_order.join(","),
                table.render()
            );

            context.emit(&json!(fingerprint), &plain);
            Ok(())
        }
    }
}

fn load_bytes(path: &Path, hex_encoded: bool) -> Result<Vec<u8>> {
    if hex_encoded {
        let text = read_text(path)?;
        let cleaned: String = text.chars().filter(|ch| ch.is_ascii_hexdigit()).collect();
        return hex::decode(cleaned)
            .map_err(|error| Error::msg(format!("{} is not hex: {error}", path.display())));
    }

    read_bytes(path)
}

pub fn diff(context: &Context, before: &Path, after: &Path, raw: bool) -> Result<()> {
    let left = read_json(before)?;
    let right = read_json(after)?;
    let diffs = diff_maps(&left, &right, !raw);

    context.emit(&json!(diffs), &render_diff(&diffs));
    Ok(())
}

pub fn baseline(context: &Context, name: &str, map: &Path) -> Result<()> {
    let value = read_json(map)?;
    let baseline = Baseline::new(name, value);
    let path = baseline.save(&context.workspace.baselines())?;

    context.emit(
        &json!({ "saved": path.display().to_string() }),
        &format!("saved {}\n", path.display()),
    );

    Ok(())
}

pub fn markers(context: &Context) -> Result<()> {
    let markers = automation_markers();

    let mut table = Table::new(&["marker", "group", "what it plants"]);
    for marker in &markers {
        table.push(vec![
            marker.name.clone(),
            marker.group.clone(),
            marker.note.clone(),
        ]);
    }

    context.emit(&json!(markers), &table.render());
    Ok(())
}

pub fn sweep(
    context: &Context,
    baselines: &[PathBuf],
    arms: &[String],
    pointer: Option<String>,
) -> Result<()> {
    if baselines.len() < 2 {
        return Err(Error::msg(
            "give at least two --baseline payloads so the noise floor can be measured",
        ));
    }

    let address = match &pointer {
        Some(text) => Some(Address::parse(text)?),
        None => None,
    };

    let extract = |value: Value| -> Value {
        match &address {
            Some(address) => address.get(&value).cloned().unwrap_or(Value::Null),
            None => value,
        }
    };

    let mut baseline_values = Vec::with_capacity(baselines.len());
    for path in baselines {
        baseline_values.push(extract(read_json(path)?));
    }

    let mut knobs = Vec::with_capacity(arms.len());
    let mut arm_values = Vec::with_capacity(arms.len());

    for spec in arms {
        let (name, path) = spec
            .split_once('=')
            .ok_or_else(|| Error::msg(format!("--arm wants name=path, got {spec}")))?;

        knobs.push(Knob::new(name, "capture"));
        arm_values.push(extract(read_json(Path::new(path))?));
    }

    let mut baseline_index = 0usize;
    let mut arm_index = 0usize;

    let report = sweep_engine::sweep(
        &knobs,
        |knob| match knob {
            None => {
                let value = baseline_values
                    .get(baseline_index)
                    .cloned()
                    .unwrap_or(Value::Null);
                baseline_index += 1;
                Ok(value)
            }
            Some(_) => {
                let value = arm_values.get(arm_index).cloned().unwrap_or(Value::Null);
                arm_index += 1;
                Ok(value)
            }
        },
        SweepOptions {
            baseline_runs: baseline_values.len(),
            ..SweepOptions::default()
        },
    )?;

    let plain = format!(
        "{}\n\n{}\n{}",
        report.summary(),
        render_arms(&report),
        render_signal_map(&report)
    );

    context.emit(&json!(report), &plain);
    Ok(())
}

pub fn verify(context: &Context, target: Option<String>, capture: Option<PathBuf>) -> Result<()> {
    let manifest = match &target {
        Some(name) => Some(target_cmd::load(context, name)?),
        None => None,
    };

    let mut suite = Acceptance::new();

    if let Some(manifest) = manifest.clone() {
        let name = manifest.name.clone();
        suite = suite.check("manifest validates", "every pattern compiles", move || {
            manifest.validate()?;
            Ok(format!("{name} is well formed"))
        });
    }

    suite = suite.check(
        "pass registry is consistent",
        "every pass has a unique name and a description",
        || {
            let mut names: Vec<&str> = wre_js::REGISTRY.iter().map(|pass| pass.name).collect();
            let before = names.len();
            names.sort_unstable();
            names.dedup();

            if before != names.len() {
                return Err(Error::msg("two passes share a name"));
            }

            if wre_js::REGISTRY.iter().any(|pass| pass.description.is_empty()) {
                return Err(Error::msg("a pass has no description"));
            }

            Ok(format!("{before} passes"))
        },
    );

    suite = suite.check(
        "the pipeline is idempotent on clean input",
        "running twice changes nothing the second time",
        || {
            let source = "var a = 1;\nfunction go(b) {\n  return a + b;\n}\n";
            let first = wre_js::deobfuscate(source, wre_js::Config::structural())?;
            let second = wre_js::deobfuscate(&first.code, wre_js::Config::structural())?;

            if first.code != second.code {
                return Err(Error::msg("a second run kept changing the source"));
            }

            Ok("stable".to_string())
        },
    );

    if let Some(path) = capture {
        let bundle = CaptureBundle::read(&path)?;
        let dir = path.clone();
        let count = bundle.requests.len();

        suite = suite.check(
            "capture bundle loads",
            "the pinned bundle parses under the current schema",
            move || Ok(format!("{count} requests")),
        );

        let posts: Vec<Vec<u8>> = bundle
            .posts()
            .into_iter()
            .filter_map(|request| request.request_body.load(&dir).ok())
            .filter(|bytes| !bytes.is_empty())
            .collect();

        suite = suite.check(
            "captured json bodies round trip",
            "every json post decodes and re-encodes",
            move || {
                let mut checked = 0usize;

                for body in &posts {
                    let mut codec: Box<dyn Codec> = Box::new(JsonCodec);
                    let report = verify_roundtrip(codec.as_mut(), body);
                    if report.opened {
                        checked += 1;
                        if !report.identical {
                            return Err(Error::msg(
                                "a json body did not reproduce its original bytes",
                            ));
                        }
                    }
                }

                Ok(format!("{checked} bodies"))
            },
        );
    }

    let report = suite.run();
    context.emit(&json!(report), &report.render());

    if !report.ok() {
        return Err(Error::msg(format!(
            "{} of {} checks failed",
            report.failed(),
            report.outcomes.len()
        )));
    }

    Ok(())
}
