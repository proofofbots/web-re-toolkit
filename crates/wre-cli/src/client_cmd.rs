use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use wre_client::sidecar::{Sidecar, SidecarOptions};
use wre_client::spec::BundleDescriptor;
use wre_codegen::binaries::Binaries;
use wre_codegen::{Language, PackageConfig, Plan, emit_all};
use wre_core::error::{Error, Result, io};

use crate::Context;
use crate::args::ClientCommand;

const CLIENTS_FILE: &str = "clients.toml";

#[derive(Debug, Default, Deserialize)]
struct ClientsFile {
    #[serde(default)]
    bundle: BTreeMap<String, BundleSpec>,
    #[serde(default)]
    package: PackageConfig,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct BundleSpec {
    #[serde(default)]
    targets: Vec<String>,
    #[serde(default)]
    features: Vec<String>,
    #[serde(default)]
    platforms: Vec<String>,
}

pub fn run(context: &Context, command: ClientCommand) -> Result<()> {
    match command {
        ClientCommand::New { id, summary, force } => new_client(context, &id, summary, force),
        ClientCommand::Bundles => bundles(context),
        ClientCommand::List { bin } => list(context, bin),
        ClientCommand::Describe { target, bin } => describe(context, target, bin),
        ClientCommand::Schema { bin, out } => schema(context, bin, out),
        ClientCommand::Build { bundle, platform, sign, zig, debug } => {
            build(context, &bundle, platform, sign, zig, debug)
        }
        ClientCommand::Package { bundle, lang, version, bin, out } => {
            package(context, &bundle, lang, version, bin, out)
        }
        ClientCommand::Test { target, bin, suite, lang } => {
            test(context, target, bin, suite, lang)
        }
        ClientCommand::Publish { bundle, lang } => publish(context, &bundle, lang),
        ClientCommand::Diag { path } => diag(context, &path),
    }
}

fn load(context: &Context) -> Result<ClientsFile> {
    let path = context.workspace.root.join(CLIENTS_FILE);
    if !path.is_file() {
        return Ok(ClientsFile::default());
    }

    let text = std::fs::read_to_string(&path).map_err(io(&path))?;
    toml::from_str(&text)
        .map_err(|error| Error::msg(format!("{} is not valid: {error}", path.display())))
}

fn bundle_spec(file: &ClientsFile, name: &str) -> Result<BundleSpec> {
    if let Some(found) = file.bundle.get(name) {
        return Ok(found.clone());
    }

    if name == "default" && file.bundle.is_empty() {
        return Ok(BundleSpec::default());
    }

    Err(Error::msg(format!(
        "no bundle {name} in {CLIENTS_FILE}, it has {}",
        if file.bundle.is_empty() {
            "none".to_string()
        } else {
            file.bundle.keys().cloned().collect::<Vec<_>>().join(", ")
        }
    )))
}

fn dist_root(context: &Context, bundle: &str) -> PathBuf {
    context.workspace.root.join("dist").join(bundle)
}

fn find_binary(context: &Context, explicit: Option<PathBuf>, bundle: &str) -> Result<PathBuf> {
    if let Some(path) = explicit {
        if path.is_file() {
            return Ok(path);
        }
        return Err(Error::msg(format!("{} is not a file", path.display())));
    }

    let name = if cfg!(windows) { "wred.exe" } else { "wred" };
    let host = host_triple().unwrap_or_default();

    let candidates = [
        dist_root(context, bundle).join("bin").join(&host).join(name),
        context.workspace.root.join("target").join("release").join(name),
        context.workspace.root.join("target").join("debug").join(name),
    ];

    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    Err(Error::msg(
        "no wred binary found, run wre client build or pass --bin",
    ))
}

fn host_triple() -> Option<String> {
    let output = Command::new("rustc").arg("-vV").output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(|value| value.trim().to_string())
}

fn describe_with(binary: &Path) -> Result<BundleDescriptor> {
    let output = Command::new(binary)
        .arg("--describe")
        .output()
        .map_err(|error| Error::msg(format!("{} did not run: {error}", binary.display())))?;

    if !output.status.success() {
        return Err(Error::msg(format!(
            "{} --describe failed: {}",
            binary.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|error| Error::msg(format!("descriptor was not json: {error}")))
}

fn bundles(context: &Context) -> Result<()> {
    let file = load(context)?;

    if file.bundle.is_empty() {
        context.note("no clients.toml, the default bundle builds every target feature");
        return Ok(());
    }

    let mut rows = String::new();
    let mut listed = Vec::new();

    for (name, spec) in &file.bundle {
        rows.push_str(&format!(
            "{name}\n  targets   {}\n  platforms {}\n",
            join(&spec.targets),
            join(&spec.platforms)
        ));
        listed.push(json!({
            "name": name,
            "targets": spec.targets,
            "platforms": spec.platforms,
            "features": spec.features,
        }));
    }

    context.emit(&json!({ "bundles": listed }), &rows);
    Ok(())
}

fn list(context: &Context, bin: Option<PathBuf>) -> Result<()> {
    let binary = find_binary(context, bin, "default")?;
    let descriptor = describe_with(&binary)?;

    let mut rows = String::new();
    for client in &descriptor.clients {
        rows.push_str(&format!(
            "{:<16} {:<8} {:>3} ops  {}\n",
            client.id,
            client.version,
            client.ops.len(),
            client.summary
        ));
    }

    context.emit(
        &json!({
            "binary": binary.display().to_string(),
            "bundle": descriptor.bundle,
            "schema_hash": descriptor.schema_hash(),
            "targets": descriptor.clients.iter().map(|client| client.id.clone()).collect::<Vec<_>>(),
        }),
        &rows,
    );

    Ok(())
}

fn describe(context: &Context, target: Option<String>, bin: Option<PathBuf>) -> Result<()> {
    let binary = find_binary(context, bin, "default")?;
    let descriptor = describe_with(&binary)?;

    let clients = match &target {
        Some(id) => vec![descriptor.find(id).cloned().ok_or_else(|| {
            Error::msg(format!(
                "no target {id} in this binary, it has {}",
                join(&descriptor.clients.iter().map(|c| c.id.clone()).collect::<Vec<_>>())
            ))
        })?],
        None => descriptor.clients.clone(),
    };

    let mut rows = String::new();
    for client in &clients {
        rows.push_str(&format!(
            "{} {} schema {}\n  {}\n  needs v8 {} chrome {} network {}, {} concurrency, warmup {}ms\n",
            client.id,
            client.version,
            descriptor.schema_hash(),
            client.summary,
            client.capabilities.needs_v8,
            client.capabilities.needs_chrome,
            client.capabilities.needs_network,
            format!("{:?}", client.capabilities.concurrency).to_lowercase(),
            client.capabilities.warmup_ms,
        ));

        for op in &client.ops {
            rows.push_str(&format!("  op {:<12} {}\n", op.name, op.summary));
        }

        for event in &client.events {
            rows.push_str(&format!("  event {:<9} {}\n", event.name, event.summary));
        }
    }

    context.emit(&json!({ "clients": clients }), &rows);
    Ok(())
}

fn schema(context: &Context, bin: Option<PathBuf>, out: Option<PathBuf>) -> Result<()> {
    let binary = find_binary(context, bin, "default")?;
    let descriptor = describe_with(&binary)?;
    let text = serde_json::to_string_pretty(&descriptor)
        .map_err(|error| Error::msg(format!("descriptor did not serialise: {error}")))?;

    match out {
        Some(path) => {
            crate::write_text(&path, &text)?;
            context.note(&format!("wrote {}", path.display()));
        }
        None => println!("{text}"),
    }

    Ok(())
}

fn build(
    context: &Context,
    bundle: &str,
    platform: Vec<String>,
    sign: bool,
    zig: bool,
    debug: bool,
) -> Result<()> {
    let file = load(context)?;
    let spec = bundle_spec(&file, bundle)?;

    let platforms = if !platform.is_empty() {
        platform
    } else if !spec.platforms.is_empty() {
        spec.platforms.clone()
    } else {
        vec![host_triple().ok_or_else(|| Error::msg("rustc did not report a host triple"))?]
    };

    let mut features: Vec<String> =
        spec.targets.iter().map(|target| format!("target-{target}")).collect();
    features.extend(spec.features.iter().cloned());

    let profile = if debug { "debug" } else { "release" };
    let out_root = dist_root(context, bundle).join("bin");
    let host = host_triple().unwrap_or_default();
    let mut written = Vec::new();

    for triple in &platforms {
        let mut command = Command::new(if zig { "cargo-zigbuild" } else { "cargo" });
        if zig {
            command.arg("zigbuild");
        } else {
            command.arg("build");
        }

        command.arg("-p").arg("wre-clientd");

        if !debug {
            command.arg("--release");
        }

        if !features.is_empty() {
            command.arg("--no-default-features");
            command.arg("--features").arg(features.join(","));
        }

        if triple != &host {
            command.arg("--target").arg(triple);
        }

        command.env("WRE_BUNDLE", bundle);
        command.current_dir(&context.workspace.root);

        context.note(&format!("building {triple}"));

        let status = command
            .status()
            .map_err(|error| Error::msg(format!("cargo did not run: {error}")))?;

        if !status.success() {
            return Err(Error::msg(format!("cargo failed for {triple}")));
        }

        let name = if triple.contains("windows") { "wred.exe" } else { "wred" };
        let built = if triple == &host {
            context.workspace.root.join("target").join(profile).join(name)
        } else {
            context.workspace.root.join("target").join(triple).join(profile).join(name)
        };

        if !built.is_file() {
            return Err(Error::msg(format!("cargo reported success but {} is missing", built.display())));
        }

        let target_path = out_root.join(triple).join(name);
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        std::fs::copy(&built, &target_path).map_err(io(&built))?;

        if sign && triple.contains("apple") && cfg!(target_os = "macos") {
            let status = Command::new("codesign")
                .arg("--force")
                .arg("--sign")
                .arg("-")
                .arg(&target_path)
                .status()
                .map_err(|error| Error::msg(format!("codesign did not run: {error}")))?;

            if !status.success() {
                return Err(Error::msg(format!("codesign failed for {}", target_path.display())));
            }
        }

        let bytes = std::fs::read(&target_path).map_err(io(&target_path))?;
        written.push(json!({
            "triple": triple,
            "path": target_path.display().to_string(),
            "bytes": bytes.len(),
            "sha256": wre_core::digest::sha256(&bytes),
        }));
    }

    let plain = written
        .iter()
        .map(|entry| {
            format!(
                "{} {} bytes\n",
                entry["path"].as_str().unwrap_or_default(),
                entry["bytes"]
            )
        })
        .collect::<String>();

    context.emit(&json!({ "bundle": bundle, "binaries": written }), &plain);
    Ok(())
}

fn package(
    context: &Context,
    bundle: &str,
    lang: Vec<String>,
    version: Option<String>,
    bin: Option<PathBuf>,
    out: Option<PathBuf>,
) -> Result<()> {
    let file = load(context)?;
    let languages = Language::parse_list(&lang)?;

    let root = dist_root(context, bundle);
    let binaries = Binaries::collect(&root.join("bin"))?;

    let binary = match bin {
        Some(path) => path,
        None => binaries
            .entries
            .iter()
            .find(|entry| Some(entry.triple.clone()) == host_triple())
            .map(|entry| entry.path.clone())
            .map_or_else(|| find_binary(context, None, bundle), Ok)?,
    };

    let descriptor = describe_with(&binary)?;

    let mut config = file.package.clone();
    if let Some(version) = version {
        config.version = version;
    }
    if config.rust_runtime_path.is_none() {
        config.rust_runtime_path = Some(
            context
                .workspace
                .root
                .join("crates")
                .join("wre-client")
                .display()
                .to_string(),
        );
    }
    if config.go_runtime_replace.is_none() {
        config.go_runtime_replace = Some(
            context
                .workspace
                .root
                .join("packages")
                .join("go")
                .join("wre")
                .display()
                .to_string(),
        );
    }

    let out_root = out.unwrap_or_else(|| root.join("packages"));
    let mut written = Vec::new();

    for client in &descriptor.clients {
        let plan = Plan {
            bundle: &descriptor,
            client,
            config: &config,
            binaries: &binaries,
            out: out_root.clone(),
        };

        for emitted in emit_all(&languages, &plan)? {
            written.push(json!({
                "target": client.id,
                "language": emitted.language.name(),
                "root": emitted.root.display().to_string(),
                "files": emitted.files.len(),
            }));
        }
    }

    if binaries.is_empty() {
        context.note("no binaries in dist, the packages carry no wred and need WRE_BINARY");
    }

    let plain = written
        .iter()
        .map(|entry| {
            format!(
                "{:<8} {:<12} {}\n",
                entry["language"].as_str().unwrap_or_default(),
                entry["target"].as_str().unwrap_or_default(),
                entry["root"].as_str().unwrap_or_default()
            )
        })
        .collect::<String>();

    context.emit(
        &json!({
            "bundle": bundle,
            "schema_hash": descriptor.schema_hash(),
            "binaries": binaries.triples(),
            "packages": written,
        }),
        &plain,
    );

    Ok(())
}

#[derive(Debug, Deserialize)]
struct Suite {
    target: String,
    #[serde(default)]
    config: Value,
    #[serde(default)]
    diag: Value,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    op: String,
    #[serde(default)]
    params: Value,
    #[serde(default)]
    expect: Value,
    #[serde(default)]
    expect_keys: Vec<String>,
    #[serde(default)]
    expect_error: Option<String>,
    #[serde(default)]
    deadline_ms: Option<u64>,
}

fn test(
    context: &Context,
    target: Option<String>,
    bin: Option<PathBuf>,
    suite: Option<PathBuf>,
    lang: Vec<String>,
) -> Result<()> {
    let binary = find_binary(context, bin, "default")?;
    let descriptor = describe_with(&binary)?;

    let ids = match target {
        Some(id) => vec![id],
        None => descriptor.clients.iter().map(|client| client.id.clone()).collect(),
    };

    let languages = if lang.is_empty() {
        vec![Language::Rust]
    } else {
        Language::parse_list(&lang)?
    };

    let mut results = Vec::new();
    let mut failures = 0usize;
    let mut plain = String::new();

    for id in &ids {
        let path = match &suite {
            Some(path) => path.clone(),
            None => context.workspace.root.join("conformance").join(format!("{id}.json")),
        };

        if !path.is_file() {
            plain.push_str(&format!("skip {id}, no suite at {}\n", path.display()));
            continue;
        }

        let text = std::fs::read_to_string(&path).map_err(io(&path))?;
        let parsed: Suite = serde_json::from_str(&text).map_err(wre_core::error::json(&path))?;

        for language in &languages {
            let outcome = match language {
                Language::Rust => run_suite_rust(&binary, &parsed),
                other => run_suite_external(context, &binary, &path, *other),
            }?;

            let failed = outcome["failed"].as_u64().unwrap_or(0) as usize;
            failures += failed;

            plain.push_str(&format!(
                "{:<7} {:<12} {} passed, {} failed\n",
                language.name(),
                parsed.target,
                outcome["passed"],
                outcome["failed"]
            ));

            for detail in outcome["cases"].as_array().unwrap_or(&Vec::new()) {
                if detail["ok"].as_bool() != Some(true) {
                    plain.push_str(&format!(
                        "  {} {}\n",
                        detail["name"].as_str().unwrap_or_default(),
                        detail["problem"].as_str().unwrap_or_default()
                    ));
                }
            }

            results.push(outcome);
        }
    }

    context.emit(&json!({ "results": results, "failed": failures }), &plain);

    if failures > 0 {
        return Err(Error::msg(format!("{failures} conformance cases failed")));
    }

    Ok(())
}

fn run_suite_rust(binary: &Path, suite: &Suite) -> Result<Value> {
    let sidecar = Sidecar::spawn(SidecarOptions::new(binary))
        .map_err(|error| Error::msg(error.to_string()))?;

    let session = sidecar
        .open_with_diag(&suite.target, suite.config.clone(), suite.diag.clone())
        .map_err(|error| Error::msg(error.to_string()))?;

    let mut cases = Vec::new();
    let mut passed = 0usize;
    let mut failed = 0usize;

    for case in &suite.cases {
        let deadline = Duration::from_millis(case.deadline_ms.unwrap_or(60_000));
        let outcome = session.call_within(&case.op, case.params.clone(), deadline);
        let problem = check(case, outcome);

        match problem {
            None => {
                passed += 1;
                cases.push(json!({ "name": case.name, "ok": true }));
            }
            Some(problem) => {
                failed += 1;
                cases.push(json!({ "name": case.name, "ok": false, "problem": problem }));
            }
        }
    }

    let _ = session.close();

    Ok(json!({
        "language": "rust",
        "target": suite.target,
        "passed": passed,
        "failed": failed,
        "cases": cases,
    }))
}

fn check(case: &Case, outcome: wre_client::error::ClientResult<Value>) -> Option<String> {
    match outcome {
        Err(error) => match &case.expect_error {
            Some(kind) if error.kind.as_str() == kind => None,
            Some(kind) => Some(format!("expected {kind}, got {}: {}", error.kind, error.message)),
            None => Some(format!("failed: {error}")),
        },
        Ok(value) => {
            if let Some(kind) = &case.expect_error {
                return Some(format!("expected {kind}, the call succeeded"));
            }

            if !case.expect.is_null() {
                if let Some(expected) = case.expect.as_object() {
                    for (key, wanted) in expected {
                        match value.get(key) {
                            Some(found) if found == wanted => {}
                            Some(found) => {
                                return Some(format!("{key} is {found}, expected {wanted}"));
                            }
                            None => return Some(format!("{key} is missing from the result")),
                        }
                    }
                } else if &case.expect != &value {
                    return Some(format!("result is {value}, expected {}", case.expect));
                }
            }

            for key in &case.expect_keys {
                if value.get(key).is_none() {
                    return Some(format!("{key} is missing from the result"));
                }
            }

            None
        }
    }
}

fn run_suite_external(
    context: &Context,
    binary: &Path,
    suite: &Path,
    language: Language,
) -> Result<Value> {
    let root = context.workspace.root.join("packages");

    let suite = suite.canonicalize().unwrap_or_else(|_| suite.to_path_buf());

    let (program, args, cwd) = match language {
        Language::Node => (
            "node".to_string(),
            vec![
                root.join("node").join("conformance").join("run.js").display().to_string(),
                suite.display().to_string(),
            ],
            root.join("node"),
        ),
        Language::Python => (
            "python3".to_string(),
            vec![
                root.join("python").join("conformance").join("run.py").display().to_string(),
                suite.display().to_string(),
            ],
            root.join("python"),
        ),
        Language::Go => (
            "go".to_string(),
            vec![
                "run".to_string(),
                "./conformance".to_string(),
                suite.display().to_string(),
            ],
            root.join("go").join("wre"),
        ),
        Language::Rust => return Err(Error::msg("rust runs in process")),
    };

    let binary = binary.canonicalize().unwrap_or_else(|_| binary.to_path_buf());

    let output = Command::new(&program)
        .args(&args)
        .current_dir(&cwd)
        .env("WRE_BINARY", &binary)
        .output()
        .map_err(|error| Error::msg(format!("{program} did not run: {error}")))?;

    let text = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(text.trim()).map_err(|error| {
        Error::msg(format!(
            "{program} did not print a json summary: {error}\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    })?;

    Ok(parsed)
}

fn diag(context: &Context, path: &Path) -> Result<()> {
    let text = std::fs::read_to_string(path).map_err(io(path))?;
    let report: Value = serde_json::from_str(&text).map_err(wre_core::error::json(path))?;

    let mut plain = String::new();
    plain.push_str(&format!(
        "{} {} session {} for {}ms\n",
        report["target"].as_str().unwrap_or("unknown"),
        report["client_version"].as_str().unwrap_or_default(),
        report["session"].as_str().unwrap_or_default(),
        report["session_ms"]
    ));

    plain.push_str(&format!(
        "reason {} at {}\n",
        report["reason"].as_str().unwrap_or_default(),
        report["generated_at"].as_str().unwrap_or_default()
    ));

    plain.push_str(&format!(
        "host bundle {} binary {} schema {}\n",
        report["host"]["bundle"].as_str().unwrap_or_default(),
        report["host"]["binary_version"].as_str().unwrap_or_default(),
        report["host"]["schema_hash"].as_str().unwrap_or_default()
    ));

    if let Some(failure) = report.get("failure").filter(|value| !value.is_null()) {
        plain.push_str(&format!(
            "failure {} {}\n",
            failure["kind"].as_str().unwrap_or_default(),
            failure["message"].as_str().unwrap_or_default()
        ));
    }

    plain.push_str(&format!(
        "calls {} failed {} events {} dropped {}\n",
        report["calls"]["total"],
        report["calls"]["failed"],
        report["events"].as_array().map(|items| items.len()).unwrap_or(0),
        report["dropped_events"]
    ));

    if let Some(events) = report["events"].as_array() {
        for event in events.iter().rev().take(12).rev() {
            plain.push_str(&format!(
                "  {:>7}ms {:<12} {:<10} {}\n",
                event["at_ms"],
                event["kind"].as_str().unwrap_or_default(),
                event["op"].as_str().unwrap_or_default(),
                event["message"].as_str().unwrap_or_default()
            ));
        }
    }

    context.emit(&report, &plain);
    Ok(())
}

fn join(values: &[String]) -> String {
    if values.is_empty() { "none".to_string() } else { values.join(", ") }
}

const SKELETON: &str = r####"use std::time::Instant;

use serde::Deserialize;
use serde_json::{Value, json};

use wre_client::client::{Client, Registration};
use wre_client::context::{Call, Ctx};
use wre_client::error::{ClientError, ClientResult};
use wre_client::shape::{Shape, field};
use wre_client::spec::{Capabilities, ClientDescriptor, Concurrency, EventSpec, OpSpec};

pub const ID: &str = "__ID__";

pub fn registration() -> Registration {
    Registration { id: ID, describe, build }
}

pub fn describe() -> ClientDescriptor {
    ClientDescriptor::new(ID, env!("CARGO_PKG_VERSION"))
        .summary("__SUMMARY__")
        .capabilities(Capabilities {
            needs_v8: false,
            needs_chrome: false,
            needs_network: true,
            stateful: true,
            concurrency: Concurrency::PerSession,
            warmup_ms: 0,
        })
        .config(Shape::object(
            "__PASCAL__Config",
            [
                field("endpoint", Shape::optional(Shape::Str)),
                field("proxy", Shape::optional(Shape::Str)),
                field("timeout_ms", Shape::Int).with_default(json!(30_000)),
            ],
        ))
        .op(
            OpSpec::new(
                "info",
                Shape::object("InfoInput", []),
                Shape::object(
                    "Info",
                    [field("target", Shape::Str), field("version", Shape::Str)],
                ),
            )
            .summary("What this build is"),
        )
        .op(
            OpSpec::new(
                "solve",
                Shape::object(
                    "Facts",
                    [
                        field("url", Shape::Str),
                        field("extra", Shape::optional(Shape::map(Shape::Json))),
                    ],
                ),
                Shape::object(
                    "Solved",
                    [field("body", Shape::Str), field("headers", Shape::map(Shape::Str))],
                ),
            )
            .summary("Produce a payload for one request")
            .deadline_ms(20_000)
            .streams(&["progress"]),
        )
        .event(EventSpec::new(
            "progress",
            Shape::object(
                "Progress",
                [
                    field("done", Shape::Int),
                    field("total", Shape::Int),
                    field("note", Shape::Str),
                ],
            ),
        ))
}

#[derive(Debug, Clone, Deserialize)]
struct Config {
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default)]
    proxy: Option<String>,
    #[serde(default = "default_timeout")]
    timeout_ms: u64,
}

fn default_timeout() -> u64 {
    30_000
}

fn build(ctx: Ctx, config: Value) -> ClientResult<Box<dyn Client>> {
    let config: Config = serde_json::from_value(config)
        .map_err(|error| ClientError::bad_input(format!("config rejected: {error}")))?;

    ctx.fact("endpoint", json!(config.endpoint));
    ctx.fact("timeout_ms", json!(config.timeout_ms));

    Ok(Box::new(__PASCAL__ { ctx, config, calls: 0 }))
}

struct __PASCAL__ {
    ctx: Ctx,
    config: Config,
    calls: u64,
}

impl Client for __PASCAL__ {
    fn call(&mut self, op: &str, params: Value, call: &Call) -> ClientResult<Value> {
        call.check()?;
        let started = Instant::now();
        self.calls += 1;

        let outcome = match op {
            "info" => Ok(json!({ "target": ID, "version": env!("CARGO_PKG_VERSION") })),

            "solve" => {
                let url = params
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ClientError::bad_input("url is required"))?
                    .to_string();

                call.progress(1, 1, "solving");
                call.debug("request", json!({ "url": url, "endpoint": self.config.endpoint }));

                Err(ClientError::unsupported(
                    "solve has no implementation yet, write it in clients/__ID__/src/lib.rs",
                ))
            }

            other => Err(ClientError::unsupported(format!("{ID} has no op {other}"))),
        };

        self.ctx
            .metric(&format!("{ID}.{op}.ms"), started.elapsed().as_millis() as f64);

        outcome.map_err(|error| error.with_op(op).with_target(ID))
    }

    fn health(&mut self) -> ClientResult<Value> {
        Ok(json!({ "ok": true, "target": ID, "detail": { "calls": self.calls } }))
    }

    fn diagnostics(&mut self) -> Value {
        json!({
            "calls": self.calls,
            "endpoint": self.config.endpoint,
            "proxy_set": self.config.proxy.is_some(),
            "timeout_ms": self.config.timeout_ms,
        })
    }
}
"####;

const SKELETON_CARGO: &str = r####"[package]
name = "wre-client-__ID__"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "__SUMMARY__"

[dependencies]
wre-client.workspace = true
wre-core.workspace = true

serde.workspace = true
serde_json.workspace = true
tracing.workspace = true
"####;

const SKELETON_SUITE: &str = r####"{
  "target": "__ID__",
  "config": {},
  "cases": [
    {
      "name": "info reports the target",
      "op": "info",
      "params": {},
      "expect": { "target": "__ID__" }
    },
    {
      "name": "solve is not written yet",
      "op": "solve",
      "params": { "url": "https://__ID__.example/" },
      "expect_error": "unsupported"
    }
  ]
}
"####;

fn new_client(
    context: &Context,
    id: &str,
    summary: Option<String>,
    force: bool,
) -> Result<()> {
    let clean = id.trim().to_lowercase();

    let usable = !clean.is_empty()
        && clean.chars().next().is_some_and(|first| first.is_ascii_lowercase())
        && clean.chars().all(|item| item.is_ascii_lowercase() || item.is_ascii_digit() || item == '-');

    if !usable {
        return Err(Error::msg(format!(
            "{id} is not a usable target id, use lowercase letters, digits and dashes"
        )));
    }

    let pascal = clean
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<String>();

    let summary =
        summary.unwrap_or_else(|| format!("Headless client for the {clean} protection"));

    let root = context.workspace.root.join("clients").join(&clean);
    let lib = root.join("src").join("lib.rs");
    let manifest = root.join("Cargo.toml");
    let suite = context.workspace.root.join("conformance").join(format!("{clean}.json"));

    if lib.exists() && !force {
        return Err(Error::msg(format!(
            "{} already exists, pass --force to overwrite it",
            lib.display()
        )));
    }

    let fill = |text: &str| {
        text.replace("__ID__", &clean)
            .replace("__PASCAL__", &pascal)
            .replace("__SUMMARY__", &summary)
    };

    crate::write_text(&manifest, &fill(SKELETON_CARGO))?;
    crate::write_text(&lib, &fill(SKELETON))?;

    if !suite.exists() || force {
        crate::write_text(&suite, &fill(SKELETON_SUITE))?;
    }

    let wired = wire_in(context, &clean)?;

    let plain = format!(
        "wrote {}\nwrote {}\nwrote {}\n{}\nnext: cargo check -p wre-client-{clean}, then wre client test {clean}\n",
        manifest.display(),
        lib.display(),
        suite.display(),
        wired.join("\n")
    );

    context.emit(
        &json!({
            "target": clean,
            "crate": format!("wre-client-{clean}"),
            "files": [manifest.display().to_string(), lib.display().to_string(), suite.display().to_string()],
            "wiring": wired,
        }),
        &plain,
    );

    Ok(())
}

fn wire_in(context: &Context, id: &str) -> Result<Vec<String>> {
    let mut done = Vec::new();

    let workspace = context.workspace.root.join("Cargo.toml");
    let text = std::fs::read_to_string(&workspace).map_err(io(&workspace))?;
    let entry = format!("wre-client-{id} = {{ path = \"clients/{id}\", version = \"0.1.0\" }}");

    if !text.contains(&entry) {
        let anchor = "wre-client = { path = \"crates/wre-client\", version = \"0.1.0\" }";
        if let Some(position) = text.find(anchor) {
            let cut = position + anchor.len();
            let updated = format!("{}\n{entry}{}", &text[..cut], &text[cut..]);
            std::fs::write(&workspace, updated).map_err(io(&workspace))?;
            done.push(format!("added {entry} to Cargo.toml"));
        } else {
            done.push(format!("add this to Cargo.toml workspace.dependencies: {entry}"));
        }
    }

    let manifest = context.workspace.root.join("crates").join("wre-clientd").join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).map_err(io(&manifest))?;
    let feature = format!("target-{id} = [\"dep:wre-client-{id}\"]");
    let dependency = format!("wre-client-{id} = {{ workspace = true, optional = true }}");
    let mut updated = text.clone();

    if !updated.contains(&feature) {
        let anchor = "target-example = [\"dep:wre-client-example\"]";
        if let Some(position) = updated.find(anchor) {
            let cut = position + anchor.len();
            updated = format!("{}\n{feature}{}", &updated[..cut], &updated[cut..]);
            done.push(format!("added feature {feature}"));
        } else {
            done.push(format!("add this feature to wre-clientd: {feature}"));
        }
    }

    if !updated.contains(&dependency) {
        let anchor = "wre-client-example = { workspace = true, optional = true }";
        if let Some(position) = updated.find(anchor) {
            let cut = position + anchor.len();
            updated = format!("{}\n{dependency}{}", &updated[..cut], &updated[cut..]);
            done.push(format!("added dependency {dependency}"));
        } else {
            done.push(format!("add this dependency to wre-clientd: {dependency}"));
        }
    }

    if updated != text {
        std::fs::write(&manifest, updated).map_err(io(&manifest))?;
    }

    let registry = context.workspace.root.join("crates").join("wre-clientd").join("src").join("registry.rs");
    let text = std::fs::read_to_string(&registry).map_err(io(&registry))?;
    let line = format!("wre_client_{}::registration()", id.replace('-', "_"));

    if !text.contains(&line) {
        let block = format!(
            "    #[cfg(feature = \"target-{id}\")]\n    registry.register({line})?;\n\n    Ok(registry)"
        );

        if let Some(position) = text.rfind("    Ok(registry)") {
            let updated = format!("{}{block}{}", &text[..position], &text[position + "    Ok(registry)".len()..]);
            std::fs::write(&registry, updated).map_err(io(&registry))?;
            done.push(format!("registered {line} behind target-{id}"));
        } else {
            done.push(format!("register {line} in wre-clientd/src/registry.rs"));
        }
    }

    done.push(format!("add \"{id}\" to a bundle in clients.toml when it is ready to ship"));

    Ok(done)
}

fn publish(context: &Context, bundle: &str, lang: Vec<String>) -> Result<()> {
    let languages = Language::parse_list(&lang)?;
    let root = dist_root(context, bundle).join("packages");

    if !root.is_dir() {
        return Err(Error::msg(format!(
            "{} does not exist, run wre client package first",
            root.display()
        )));
    }

    let mut steps = Vec::new();

    for language in &languages {
        let dir = root.join(language.name());
        if !dir.is_dir() {
            continue;
        }

        let mut targets: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(io(&dir))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        targets.sort();

        for target in targets {
            let name = target
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string();

            match language {
                Language::Node => {
                    let platforms = target.join("platform");
                    if platforms.is_dir() {
                        let mut entries: Vec<PathBuf> = std::fs::read_dir(&platforms)
                            .map_err(io(&platforms))?
                            .filter_map(|entry| entry.ok())
                            .map(|entry| entry.path())
                            .collect();
                        entries.sort();

                        for entry in entries {
                            steps.push(json!({
                                "language": "node",
                                "target": name,
                                "why": "the platform package must exist before the one that depends on it",
                                "command": format!("npm publish --access public {}", entry.display()),
                            }));
                        }
                    }

                    steps.push(json!({
                        "language": "node",
                        "target": name,
                        "why": "the package callers install",
                        "command": format!("npm publish --access public {}", target.display()),
                    }));
                }

                Language::Python => {
                    steps.push(json!({
                        "language": "python",
                        "target": name,
                        "why": "one platform tagged wheel per binary",
                        "command": format!("bash {}", target.join("build_wheels.sh").display()),
                    }));
                    steps.push(json!({
                        "language": "python",
                        "target": name,
                        "why": "upload every wheel that was built",
                        "command": format!("twine upload {}", target.join("dist").join("*.whl").display()),
                    }));
                }

                Language::Go => {
                    steps.push(json!({
                        "language": "go",
                        "target": name,
                        "why": "go modules are published by tag, and the binary must be reachable at the download url",
                        "command": format!("git tag packages/go/clients/{name}/v0.1.0 && git push origin packages/go/clients/{name}/v0.1.0"),
                    }));
                }

                Language::Rust => {
                    steps.push(json!({
                        "language": "rust",
                        "target": name,
                        "why": "wre-client has to be on crates.io first, and the generated crate must not point at a local path",
                        "command": format!("cargo publish --manifest-path {}", target.join("Cargo.toml").display()),
                    }));
                }
            }
        }
    }

    let plain = steps
        .iter()
        .map(|step| {
            format!(
                "{:<7} {}\n        {}\n",
                step["language"].as_str().unwrap_or_default(),
                step["command"].as_str().unwrap_or_default(),
                step["why"].as_str().unwrap_or_default()
            )
        })
        .collect::<String>();

    context.emit(
        &json!({ "bundle": bundle, "steps": steps }),
        &format!("{plain}\nnothing was published, these are the commands to run\n"),
    );

    Ok(())
}
