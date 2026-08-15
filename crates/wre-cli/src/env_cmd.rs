use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;

use wre_cdp::chrome::{Chrome, LaunchOptions};
use wre_core::error::Result;
use wre_env::{
    CaptureOptions, MaterializeOptions, Snapshot, capture_script, materialize, synthetic_snapshot,
};
use wre_live::realm::{Realm, RealmOptions};

use crate::args::EnvCommand;
use crate::{Context, read_json, read_text, write_text};

pub async fn run(context: &Context, command: EnvCommand) -> Result<()> {
    match command {
        EnvCommand::Script { depth } => script(context, depth),
        EnvCommand::Snapshot { url, port, headless, depth, out, wait } => {
            snapshot(context, &url, port, headless, depth, out, wait).await
        }
        EnvCommand::Run { script, snapshot, expression, timeout } => {
            run_script(context, &script, snapshot, expression, timeout)
        }
    }
}

fn script(context: &Context, depth: usize) -> Result<()> {
    let options = CaptureOptions { depth, ..CaptureOptions::default() };
    let source = capture_script(&options)?;
    context.emit(&json!({ "bytes": source.len() }), &source);
    Ok(())
}

async fn snapshot(
    context: &Context,
    url: &str,
    port: u16,
    headless: bool,
    depth: usize,
    out: Option<PathBuf>,
    wait: u64,
) -> Result<()> {
    let chrome = Chrome::launch(LaunchOptions {
        port,
        headless,
        profile: context
            .workspace
            .chrome_profiles()
            .join(format!("profile-{port}")),
        ..LaunchOptions::default()
    })
    .await?;

    let session = chrome.reuse_page().await?;
    session.enable(&["Page", "Runtime"]).await?;
    session
        .navigate_and_wait(url, Duration::from_secs(wait))
        .await?;

    let options = CaptureOptions { depth, ..CaptureOptions::default() };
    let raw = session.evaluate_json(&capture_script(&options)?).await?;
    let snapshot = Snapshot::parse(&raw)?;

    let destination = out.unwrap_or_else(|| {
        context
            .workspace
            .artifact("snapshots")
            .join(format!("{}.json", wre_core::paths::safe_name(url)))
    });

    write_text(
        &destination,
        &format!("{}\n", serde_json::to_string_pretty(&raw).unwrap_or_default()),
    )?;

    session.navigate("about:blank").await.ok();

    let record = json!({
        "output": destination.display().to_string(),
        "objects": snapshot.objects.len(),
        "roots": snapshot.roots.keys().collect::<Vec<_>>(),
        "functions": snapshot.function_count(),
        "getters": snapshot.getter_count(),
        "truncated": snapshot.truncated,
        "userAgent": snapshot.user_agent,
    });

    let plain = format!(
        "wrote {}\n  {} objects, {} functions, {} getters across {} roots{}\n",
        destination.display(),
        snapshot.objects.len(),
        snapshot.function_count(),
        snapshot.getter_count(),
        snapshot.roots.len(),
        if snapshot.truncated { ", truncated at the object budget" } else { "" }
    );

    context.emit(&record, &plain);
    Ok(())
}

fn run_script(
    context: &Context,
    script: &std::path::Path,
    snapshot: Option<PathBuf>,
    expression: Option<String>,
    timeout: u64,
) -> Result<()> {
    let source = read_text(script)?;

    let mut realm = Realm::new(RealmOptions {
        timeout: Duration::from_secs(timeout),
        ..RealmOptions::default()
    })?;

    let snapshot = match snapshot {
        Some(path) => Snapshot::parse(&read_json(&path)?)?,
        None => synthetic_snapshot(),
    };

    let report = materialize(&mut realm, &snapshot, &MaterializeOptions::default())?;
    realm.eval_unit(&source, "target")?;

    let value = match expression {
        Some(expression) => realm.eval_json(&expression)?,
        None => json!(null),
    };

    let records = realm.records().unwrap_or_default();

    let record = json!({
        "roots": report.roots,
        "objects": report.objects,
        "result": value,
        "console": records.console.iter().map(|line| format!("{}: {}", line.level, line.text)).collect::<Vec<_>>(),
        "errors": records.errors.len(),
    });

    let mut plain = format!(
        "materialised {} objects across {} roots\n",
        report.objects,
        report.roots.len()
    );

    for line in &records.console {
        plain.push_str(&format!("  {}: {}\n", line.level, line.text));
    }

    if !value.is_null() {
        plain.push_str(&format!(
            "{}\n",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        ));
    }

    context.emit(&record, &plain);
    Ok(())
}
