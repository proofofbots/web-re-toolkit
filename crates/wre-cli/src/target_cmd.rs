use serde_json::json;

use wre_core::error::{Error, Result};
use wre_net::http::{Client, ClientOptions};
use wre_net::proxy::ProxySpec;
use wre_report::table::Table;
use wre_target::Manifest;

use crate::Context;

pub fn manifest_path(context: &Context, target: &str) -> std::path::PathBuf {
    context.workspace.targets().join(format!("{target}.toml"))
}

pub fn load(context: &Context, target: &str) -> Result<Manifest> {
    let path = manifest_path(context, target);
    if !path.exists() {
        return Err(Error::msg(format!(
            "no manifest at {}, run `wre init {target}` first",
            path.display()
        )));
    }
    Manifest::load(&path)
}

pub fn init(context: &Context, name: &str, url: Option<String>, force: bool) -> Result<()> {
    let path = manifest_path(context, name);

    if path.exists() && !force {
        return Err(Error::msg(format!(
            "{} already exists, pass --force to overwrite",
            path.display()
        )));
    }

    let mut manifest = Manifest::example();
    manifest.name = name.to_string();
    manifest.description = format!("adapter for {name}");

    if let Some(url) = url {
        manifest.urls = vec![url];
        manifest.pages.clear();
    }

    std::fs::create_dir_all(context.workspace.targets())
        .map_err(wre_core::error::io(context.workspace.targets()))?;

    manifest.save(&path)?;

    context.emit(
        &json!({ "wrote": path.display().to_string() }),
        &format!("wrote {}\n", path.display()),
    );

    Ok(())
}

pub fn list(context: &Context) -> Result<()> {
    let dir = context.workspace.targets();

    if !dir.exists() {
        context.emit(&json!([]), "no targets directory yet\n");
        return Ok(());
    }

    let mut rows = Vec::new();
    let mut records = Vec::new();

    for entry in std::fs::read_dir(&dir).map_err(wre_core::error::io(&dir))? {
        let entry = entry.map_err(wre_core::error::io(&dir))?;
        let path = entry.path();

        if path.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }

        match Manifest::load(&path) {
            Ok(manifest) => {
                records.push(json!({
                    "name": manifest.name,
                    "urls": manifest.urls,
                    "knobs": manifest.knobs.len(),
                    "checks": manifest.checks.len(),
                    "vm": manifest.vm.is_some(),
                }));
                rows.push(vec![
                    manifest.name.clone(),
                    manifest.urls.first().cloned().unwrap_or_default(),
                    manifest.knobs.len().to_string(),
                    if manifest.vm.is_some() { "yes".into() } else { String::new() },
                ]);
            }
            Err(error) => {
                records.push(json!({ "path": path.display().to_string(), "error": error.to_string() }));
                rows.push(vec![
                    path.display().to_string(),
                    format!("does not load: {error}"),
                    String::new(),
                    String::new(),
                ]);
            }
        }
    }

    let mut table = Table::new(&["target", "url", "knobs", "vm"]);
    for row in rows {
        table.push(row);
    }

    context.emit(&json!(records), &table.render());
    Ok(())
}

pub fn check(context: &Context, target: &str) -> Result<()> {
    let manifest = load(context, target)?;
    manifest.validate()?;

    let summary = json!({
        "name": manifest.name,
        "urls": manifest.urls.len(),
        "pages": manifest.pages.len(),
        "signatures": manifest.live.signatures.len(),
        "patches": manifest.live.patches.len(),
        "exports": manifest.live.exports.len(),
        "knobs": manifest.knobs.len(),
        "checks": manifest.checks.len(),
        "vm": manifest.vm.is_some(),
        "codec": format!("{:?}", manifest.wire.codec),
    });

    let plain = format!(
        "{} is valid\n  {} urls, {} pages\n  {} signatures, {} patches, {} exports\n  {} knobs, {} checks\n  codec {:?}, vm section {}\n",
        manifest.name,
        manifest.urls.len(),
        manifest.pages.len(),
        manifest.live.signatures.len(),
        manifest.live.patches.len(),
        manifest.live.exports.len(),
        manifest.knobs.len(),
        manifest.checks.len(),
        manifest.wire.codec,
        if manifest.vm.is_some() { "present" } else { "absent" }
    );

    context.emit(&summary, &plain);
    Ok(())
}

pub async fn discover(
    context: &Context,
    url: &str,
    target: Option<String>,
    proxy: Option<String>,
) -> Result<()> {
    let manifest = match &target {
        Some(name) => Some(load(context, name)?),
        None => None,
    };

    let proxy = match proxy {
        Some(spec) => Some(ProxySpec::parse(&spec)?),
        None => ProxySpec::from_env(),
    };

    let client = Client::new(ClientOptions { proxy, ..ClientOptions::default() })?;
    let response = client
        .fetch(wre_net::http::FetchRequest::get(url))
        .await?;

    let document = response.text();

    let scripts = match &manifest {
        Some(manifest) => manifest.discovery.find_scripts(&document)?,
        None => generic_scripts(&document),
    };

    let markers = manifest
        .as_ref()
        .map(|manifest| manifest.discovery.marks(&document))
        .unwrap_or_default();

    let cookies: Vec<String> = response
        .set_cookies()
        .into_iter()
        .map(|value| value.split(';').next().unwrap_or(value).to_string())
        .collect();

    let record = json!({
        "url": response.url,
        "status": response.status,
        "protocol": response.version,
        "bytes": response.body.len(),
        "scripts": scripts,
        "markers": markers,
        "cookies": cookies,
    });

    let mut plain = format!(
        "{} {} ({} bytes over {})\n",
        response.status,
        response.url,
        response.body.len(),
        response.version
    );

    if !scripts.is_empty() {
        plain.push_str("scripts:\n");
        for script in &scripts {
            plain.push_str(&format!("  {script}\n"));
        }
    }

    if !markers.is_empty() {
        plain.push_str("markers:\n");
        for marker in &markers {
            plain.push_str(&format!("  {marker}\n"));
        }
    }

    if !cookies.is_empty() {
        plain.push_str("cookies:\n");
        for cookie in &cookies {
            plain.push_str(&format!("  {cookie}\n"));
        }
    }

    context.emit(&record, &plain);
    Ok(())
}

fn generic_scripts(document: &str) -> Vec<String> {
    let Ok(regex) = regex_lite(r#"<script[^>]+src=["']([^"']+)["']"#) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for capture in regex.captures_iter(document) {
        if let Some(found) = capture.get(1) {
            let value = found.as_str().to_string();
            if !out.contains(&value) {
                out.push(value);
            }
        }
    }
    out
}

fn regex_lite(pattern: &str) -> std::result::Result<regex::Regex, regex::Error> {
    regex::Regex::new(pattern)
}
