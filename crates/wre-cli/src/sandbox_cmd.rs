use serde_json::{Value, json};

use wre_cdp::chrome::{Chrome, LaunchOptions};
use wre_core::error::{Error, Result, io, json as json_error};
use wre_live::realm::{Realm, RealmOptions};
use wre_sandbox::capture::{Incoming, Server, Stored, Taken};
use wre_sandbox::graph::{GraphLibrary, GraphProfile, bundled_ids};
use wre_sandbox::library::{Library, Record, now};
use wre_sandbox::{Finding, Profile, audit, install, warnings};

use crate::args::SandboxCommand;
use crate::{Context, target_cmd};

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }

    out
}

fn library(context: &Context) -> Result<Library> {
    Library::load(context.workspace.profiles())
}

fn record_of(
    context: &Context,
    profile: Option<String>,
    target: Option<String>,
    random: bool,
) -> Result<Record> {
    if let Some(name) = target {
        let manifest = target_cmd::load(context, &name)?;
        let profile = manifest.sandbox.ok_or_else(|| {
            Error::msg(format!(
                "target {name} declares no [sandbox] profile, drop --target to use the library"
            ))
        })?;

        return Ok(Record {
            id: format!("target-{name}"),
            label: format!("the [sandbox] profile in targets/{name}.toml"),
            notes: String::new(),
            captured_at: String::new(),
            origin: Default::default(),
            profile,
        });
    }

    library(context)?.resolve(profile.as_deref(), random)
}

fn derive_id(label: &str, user_agent: &str) -> String {
    if !label.is_empty() {
        return label.to_lowercase().replace(' ', "-");
    }

    let system = if user_agent.contains("Android") {
        "android"
    } else if user_agent.contains("iPhone") || user_agent.contains("iPad") {
        "ios"
    } else if user_agent.contains("Macintosh") {
        "macos"
    } else if user_agent.contains("Windows") {
        "windows"
    } else if user_agent.contains("Linux") {
        "linux"
    } else {
        "device"
    };

    let engine = ["Firefox", "Edg", "OPR", "Chrome", "Safari"]
        .into_iter()
        .find(|name| user_agent.contains(name))
        .unwrap_or("browser")
        .to_lowercase();

    format!("{system}-{engine}-{}", wre_core::paths::day())
}

fn findings_json(findings: &[Finding]) -> Value {
    Value::Array(
        findings
            .iter()
            .map(|finding| json!({ "level": finding.level.as_str(), "what": finding.what }))
            .collect(),
    )
}

fn print_findings(context: &Context, findings: &[Finding]) {
    for finding in findings {
        context.note(&format!("{}: {}", finding.level.as_str(), finding.what));
    }
}

fn check_profile(profile: &Profile) -> Result<(Vec<Value>, usize, Vec<String>, usize)> {
    let mut realm = Realm::new(RealmOptions::default())?;
    let sandbox = install(&mut realm, profile)?;

    let checks: Vec<(&str, &str, Value)> = vec![
        (
            "getters are native",
            "Object.getOwnPropertyDescriptor(Navigator.prototype,'userAgent')\
             .get.toString().indexOf('[native code]') >= 0",
            json!(true),
        ),
        (
            "toString is untouched",
            "Function.prototype.toString.toString().indexOf('[native code]') >= 0",
            json!(true),
        ),
        (
            "wrong receiver throws",
            "(function () { try { \
               Object.getOwnPropertyDescriptor(Navigator.prototype,'userAgent').get.call({}); \
               return 'no throw'; } catch (error) { return error.message; } })()",
            json!("Illegal invocation"),
        ),
        (
            "the brand tag is right",
            "Object.prototype.toString.call(navigator)",
            json!("[object Navigator]"),
        ),
        (
            "properties sit on the prototype",
            "Object.getOwnPropertyDescriptor(navigator,'userAgent') === undefined",
            json!(true),
        ),
        (
            "no fixed toolkit prefix is reachable",
            "Object.getOwnPropertyNames(globalThis)\
             .filter(function (n) { return n.indexOf('__wre') === 0; }).length",
            json!(0),
        ),
        (
            "the instrumentation is off the global",
            "Object.getOwnPropertyNames(globalThis).filter(function (n) { \
               var v; try { v = globalThis[n]; } catch (error) { return false; } \
               return v && typeof v === 'object' && typeof v.drain === 'function' \
                 && typeof v.push === 'function'; }).length",
            json!(0),
        ),
        (
            "matchMedia is native and so is what it returns",
            "matchMedia.toString().indexOf('[native code]') >= 0 && \
             Object.getOwnPropertyNames(matchMedia('(hover: hover)')).length === 0",
            json!(true),
        ),
        (
            "permissions keeps its identity",
            "navigator.permissions === navigator.permissions && \
             navigator.permissions.query.toString().indexOf('[native code]') >= 0",
            json!(true),
        ),
    ];

    let mut results = Vec::new();
    let mut failed = 0usize;

    for (label, expression, expected) in checks {
        let got = realm.eval_json(expression)?;
        let held = got == expected;

        if !held {
            failed += 1;
        }

        results.push(json!({ "check": label, "held": held, "got": got }));
    }

    Ok((results, failed, sandbox.misses(), sandbox.installed().len()))
}

pub async fn run(context: &Context, command: SandboxCommand) -> Result<()> {
    match command {
        SandboxCommand::List => {
            let library = library(context)?;
            let mut records = library.records().to_vec();
            records.push(Record::builtin());

            let mut plain =
                String::from("| id | captured | warnings | device |\n| --- | --- | --- | --- |\n");
            let mut rendered = Vec::new();

            for record in &records {
                let findings = audit(&record.profile);
                let warned = warnings(&findings);

                plain.push_str(&format!(
                    "| {} | {} | {warned} | {} |\n",
                    record.id,
                    if record.captured_at.is_empty() {
                        "built in"
                    } else {
                        &record.captured_at
                    },
                    record.summary()
                ));

                rendered.push(json!({
                    "id": record.id,
                    "label": record.label,
                    "captured_at": record.captured_at,
                    "builtin": record.is_builtin(),
                    "user_agent": record.origin.user_agent,
                    "findings": findings_json(&findings),
                }));
            }

            plain.push_str(&format!("\n{}\n", library.dir().display()));
            if library.is_empty() {
                plain.push_str("no captured profiles yet, run `wre sandbox capture`\n");
            }

            let graph_dir = context.workspace.profiles().join("graph");
            let graphs = GraphLibrary::load(&graph_dir).unwrap_or_default();
            let mut described = Vec::new();

            let mut listed = graphs.ids();
            for id in bundled_ids() {
                if !listed.contains(&id) {
                    listed.push(id);
                }
            }

            if graphs.is_empty() {
                plain.push_str("no captured graphs yet, the bundled one is used instead\n");
            }

            if !listed.is_empty() {
                plain.push_str(
                    "\n| graph | captured | objects | tables | source |\n| --- | --- | --- | --- | --- |\n",
                );

                for id in listed {
                    let Ok(profile) = graphs.resolve(Some(&id)) else {
                        continue;
                    };

                    let bundled = !graphs.ids().contains(&id);

                    plain.push_str(&format!(
                        "| {id} | {} | {} | {} | {} |\n",
                        profile.captured_at,
                        profile.objects(),
                        profile.tables.present().join(" "),
                        if bundled { "bundled" } else { "captured" }
                    ));

                    described.push(json!({
                        "id": id,
                        "label": profile.label,
                        "captured_at": profile.captured_at,
                        "user_agent": profile.user_agent,
                        "objects": profile.objects(),
                        "tables": profile.tables.present(),
                        "bundled": bundled,
                    }));
                }

                plain.push_str(&format!("\n{}\n", graph_dir.display()));
            }

            context.emit(
                &json!({
                    "profiles": rendered,
                    "graphs": described,
                    "dir": library.dir().display().to_string(),
                }),
                &plain,
            );
            Ok(())
        }

        SandboxCommand::Profile {
            profile,
            target,
            random,
        } => {
            let record = record_of(context, profile, target, random)?;
            let findings = audit(&record.profile);

            let mut plain = format!("{}\n\n", record.summary());
            plain.push_str("| interface | property | value |\n| --- | --- | --- |\n");
            for interface in &record.profile.interfaces {
                for (name, value) in &interface.properties {
                    plain.push_str(&format!(
                        "| {} | {name} | {value} |\n",
                        interface.constructor
                    ));
                }
            }

            plain.push_str(&format!(
                "\n{} plugins, {} webgl parameters, {} extensions, {} media queries, {} fonts\n",
                record.profile.plugins.len(),
                record.profile.webgl_parameters.len(),
                record.profile.webgl_extensions.len(),
                record.profile.media_queries.len(),
                record.profile.font_widths.len()
            ));

            for finding in &findings {
                plain.push_str(&format!("{}: {}\n", finding.level.as_str(), finding.what));
            }

            let rendered = serde_json::to_value(&record)
                .map_err(|error| Error::msg(format!("profile did not render: {error}")))?;

            context.emit(
                &json!({ "record": rendered, "findings": findings_json(&findings) }),
                &plain,
            );
            Ok(())
        }

        SandboxCommand::Check {
            profile,
            target,
            random,
            all,
        } => {
            let records = if all {
                let library = library(context)?;
                let mut records = library.records().to_vec();
                records.push(Record::builtin());
                records
            } else {
                vec![record_of(context, profile, target, random)?]
            };

            let mut plain = String::new();
            let mut rendered = Vec::new();
            let mut failed = 0usize;

            for record in &records {
                let (results, misfires, misses, installed) = check_profile(&record.profile)?;
                failed += misfires;

                plain.push_str(&format!(
                    "{}\n| check | verdict |\n| --- | --- |\n",
                    record.id
                ));
                for result in &results {
                    let held = result["held"].as_bool().unwrap_or(false);
                    plain.push_str(&format!(
                        "| {} | {} |\n",
                        result["check"].as_str().unwrap_or_default(),
                        if held {
                            "holds".to_string()
                        } else {
                            format!("got {}", result["got"])
                        }
                    ));
                }

                plain.push_str(&format!(
                    "\n{installed} surfaces installed, {misfires} checks failed\n"
                ));
                if !misses.is_empty() {
                    plain.push_str(&format!("misses: {}\n", misses.join(", ")));
                }
                plain.push('\n');

                rendered.push(json!({
                    "id": record.id,
                    "checks": results,
                    "misses": misses,
                    "installed": installed,
                    "failed": misfires,
                }));
            }

            context.emit(&json!({ "profiles": rendered, "failed": failed }), &plain);

            if failed > 0 {
                return Err(Error::msg(format!("{failed} sandbox checks did not hold")));
            }

            Ok(())
        }

        SandboxCommand::Capture {
            host,
            port,
            open,
            graph,
            calls,
            label,
            chrome_port,
            keep,
            force,
        } => {
            let mut library = library(context)?;
            let graph_dir = context.workspace.profiles().join("graph");
            let mut server = Server::bind(&host, port)?;

            if graph {
                server = server.walking_the_graph();
            }

            if let Some(path) = calls {
                let text = std::fs::read_to_string(&path).map_err(io(&path))?;
                context.note(&format!("answering the call list in {}", path.display()));
                server = server.answering_calls(text);
            }

            if open {
                server = server.sending_on_its_own(label.clone());
            }

            context.note(&format!("capture page on {}", server.url()));
            context.note(&format!(
                "profiles land in {}",
                if graph {
                    graph_dir.display().to_string()
                } else {
                    library.dir().display().to_string()
                }
            ));

            if host != "127.0.0.1" && host != "localhost" {
                context.note(
                    "bound off loopback: the page and the profile travel over the LAN in the clear, \
                     and crypto.subtle is unavailable outside a secure context so canvas hashes fall \
                     back to fnv1a",
                );
            }

            context.note(if keep {
                "open it in the browser you want to replay, stop with ctrl-c"
            } else {
                "open it in the browser you want to replay, this exits after one capture"
            });

            let mut driven = None;

            if open {
                let url = format!("{}/", server.url());

                let chrome = Chrome::launch(LaunchOptions {
                    port: chrome_port,
                    headless: false,
                    offscreen: false,
                    profile: context
                        .workspace
                        .chrome_profiles()
                        .join(format!("profile-{chrome_port}")),
                    ..LaunchOptions::default()
                })
                .await?;

                if chrome.headless || chrome.version.user_agent.contains("Headless") {
                    context.note(
                        "the browser on that port is headless, its profile is not one to replay",
                    );
                }

                let session = chrome.new_page(&url).await?;
                session.try_send("Page.enable", json!({})).await;
                session.try_send("Page.bringToFront", json!({})).await;
                session
                    .try_send("Page.navigate", json!({ "url": url }))
                    .await;

                context.note(&format!(
                    "driving {} on port {chrome_port}",
                    chrome.version.browser
                ));
                driven = Some((chrome, session));
            }

            let mut stored: Vec<Value> = Vec::new();

            server.run(keep, |taken: Taken, peer: &str| match taken {
                Taken::Graph(incoming) => {
                    let profile = GraphProfile {
                        id: derive_id(&incoming.label, &incoming.user_agent),
                        label: incoming.label.clone(),
                        captured_at: now(),
                        href: incoming.href.clone(),
                        user_agent: incoming.user_agent.clone(),
                        snapshot: incoming.snapshot.clone(),
                        tables: incoming.tables.clone(),
                    };

                    let id = profile.id.clone();
                    let path = GraphLibrary::store(&graph_dir, &profile, force)?;

                    context.note(&format!(
                        "stored {id} from {peer} at {}: {} objects, tables {}",
                        path.display(),
                        profile.objects(),
                        profile.tables.present().join(", ")
                    ));

                    stored.push(json!({
                        "id": id,
                        "path": path.display().to_string(),
                        "objects": profile.objects(),
                        "tables": profile.tables.present(),
                    }));

                    Ok(Stored {
                        id,
                        path: path.display().to_string(),
                        warnings: 0,
                    })
                }

                Taken::Profile(incoming) => {
                    let findings = audit(&incoming.profile);
                    let warned = warnings(&findings);

                    let mut origin = incoming.origin.clone();
                    origin.client = peer.to_string();

                    let record = Record {
                        id: derive_id(&incoming.label, &origin.user_agent),
                        label: incoming.label.clone(),
                        notes: incoming.notes.clone(),
                        captured_at: now(),
                        origin,
                        profile: incoming.profile.clone(),
                    };

                    let id = record.id.clone();
                    let path = library.store(record, force)?;

                    context.note(&format!("stored {id} from {peer} at {}", path.display()));
                    print_findings(context, &findings);

                    stored.push(json!({
                        "id": id,
                        "path": path.display().to_string(),
                        "findings": findings_json(&findings),
                    }));

                    Ok(Stored {
                        id,
                        path: path.display().to_string(),
                        warnings: warned,
                    })
                }
            })?;

            if let Some((_chrome, session)) = driven {
                session.try_send("Page.close", json!({})).await;
            }

            context.emit(&json!({ "stored": stored }), "");
            Ok(())
        }

        SandboxCommand::Import { input, id, force } => {
            let text = std::fs::read_to_string(&input).map_err(io(&input))?;
            let incoming: Incoming = serde_json::from_str(&text).map_err(json_error(&input))?;

            let findings = audit(&incoming.profile);
            let mut library = library(context)?;

            let record = Record {
                id: id.unwrap_or_else(|| derive_id(&incoming.label, &incoming.origin.user_agent)),
                label: incoming.label,
                notes: incoming.notes,
                captured_at: now(),
                origin: incoming.origin,
                profile: incoming.profile,
            };

            let stored_id = record.id.clone();
            let path = library.store(record, force)?;

            let mut plain = format!("stored {stored_id} at {}\n", path.display());
            for finding in &findings {
                plain.push_str(&format!("{}: {}\n", finding.level.as_str(), finding.what));
            }

            context.emit(
                &json!({
                    "id": stored_id,
                    "path": path.display().to_string(),
                    "findings": findings_json(&findings),
                }),
                &plain,
            );

            Ok(())
        }
    }
}
