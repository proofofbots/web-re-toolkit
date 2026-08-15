use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;

use wre_capture::CaptureOptions;
use wre_cdp::chrome::{Chrome, LaunchOptions, is_running, probe_version};
use wre_core::bundle::CaptureBundle;
use wre_core::error::{Error, Result};
use wre_core::paths::{day, safe_name};
use wre_report::table::Table;

use crate::{Context, target_cmd};

pub async fn browser(
    context: &Context,
    port: u16,
    status: bool,
    stop: bool,
    start: bool,
    headless: bool,
) -> Result<()> {
    if stop {
        if !is_running(port).await {
            context.emit(&json!({ "running": false }), "nothing running\n");
            return Ok(());
        }

        let mut chrome = Chrome::connect_existing(port).await?;
        chrome.shutdown().await?;
        context.emit(&json!({ "stopped": true, "port": port }), "stopped\n");
        return Ok(());
    }

    if start && !is_running(port).await {
        let chrome = Chrome::launch(LaunchOptions {
            port,
            headless,
            profile: context.workspace.chrome_profiles().join(format!("profile-{port}")),
            ..LaunchOptions::default()
        })
        .await?;

        context.emit(
            &json!({ "started": true, "port": port, "browser": chrome.version.browser }),
            &format!("started {} on {port}\n", chrome.version.browser),
        );
        return Ok(());
    }

    let running = is_running(port).await;

    if !running {
        context.emit(
            &json!({ "running": false, "port": port }),
            &format!("no browser on {port}\n"),
        );
        return Ok(());
    }

    let version = probe_version(port).await?;

    if status || !start {
        let chrome = Chrome::connect_existing(port).await?;
        let targets = chrome.targets().await.unwrap_or_default();

        let pages: Vec<String> = targets
            .iter()
            .filter(|target| target.kind == "page")
            .map(|target| target.url.clone())
            .collect();

        context.emit(
            &json!({
                "running": true,
                "port": port,
                "browser": version.browser,
                "protocol": version.protocol_version,
                "pages": pages,
            }),
            &format!(
                "{} on {port}, protocol {}\n{}",
                version.browser,
                version.protocol_version,
                pages
                    .iter()
                    .map(|url| format!("  {url}\n"))
                    .collect::<String>()
            ),
        );
    }

    Ok(())
}

pub struct CaptureArgs {
    pub target: Option<String>,
    pub url: Option<String>,
    pub page: Option<String>,
    pub wait: u64,
    pub headless: bool,
    pub port: u16,
    pub keep_storage: bool,
    pub proxy: Option<String>,
    pub out: Option<PathBuf>,
    pub no_probe: bool,
    pub scripts: bool,
}

pub async fn capture(context: &Context, args: CaptureArgs) -> Result<()> {
    let manifest = match &args.target {
        Some(name) => Some(target_cmd::load(context, name)?),
        None => None,
    };

    let url = args
        .url
        .clone()
        .or_else(|| {
            let manifest = manifest.as_ref()?;
            match &args.page {
                Some(page) => manifest.page(page).map(str::to_string),
                None => manifest.first_url().map(str::to_string),
            }
        })
        .ok_or_else(|| Error::msg("no url given and the manifest has none"))?;

    let name = args
        .target
        .clone()
        .unwrap_or_else(|| safe_name(&url).chars().take(40).collect());

    let out_dir = args.out.clone().unwrap_or_else(|| {
        context
            .workspace
            .artifact("captures")
            .join(format!("{name}-{}", day()))
    });

    let mut options = CaptureOptions::new(&name, &url, out_dir.clone());
    options.wait = Duration::from_secs(args.wait);
    options.headless = args.headless;
    options.port = args.port;
    options.keep_storage = args.keep_storage;
    options.proxy = args.proxy.clone();
    options.profile_dir = context
        .workspace
        .chrome_profiles()
        .join(format!("profile-{}", args.port));

    if args.scripts {
        options = options.recording_scripts();
    }

    if !args.no_probe {
        let spec = manifest
            .as_ref()
            .map(|manifest| manifest.probe.to_spec())
            .unwrap_or_else(wre_probe::fingerprint_surface);
        options = options.with_probe("surface", spec);
    }

    if let Some(manifest) = &manifest {
        for pattern in &manifest.wire.request_patterns {
            let _ = regex::Regex::new(pattern)
                .map_err(|error| Error::msg(format!("bad request pattern {pattern}: {error}")))?;
        }
    }

    let bundle = wre_capture::run(options).await?;

    let record = json!({
        "id": bundle.id,
        "dir": out_dir.display().to_string(),
        "requests": bundle.requests.len(),
        "scripts": bundle.scripts.len(),
        "cookies": bundle.cookies.len(),
        "console": bundle.console.len(),
        "exceptions": bundle.exceptions.len(),
        "probes": bundle.probes.keys().collect::<Vec<_>>(),
    });

    let plain = format!(
        "captured {} into {}\n  {} requests, {} scripts, {} cookies, {} console lines, {} exceptions\n",
        bundle.id,
        out_dir.display(),
        bundle.requests.len(),
        bundle.scripts.len(),
        bundle.cookies.len(),
        bundle.console.len(),
        bundle.exceptions.len()
    );

    context.emit(&record, &plain);
    Ok(())
}

pub fn pin(context: &Context, from: Option<PathBuf>, name: &str) -> Result<()> {
    let source = match from {
        Some(path) => path,
        None => {
            let store = wre_core::store::Store::new(context.workspace.artifact("captures"));
            store.require_newest()?
        }
    };

    let bundle = CaptureBundle::read(&source)?;
    let destination = context.workspace.capture_dir(name);
    let copied = wre_capture::pin(&bundle, &source, &destination)?;

    context.emit(
        &json!({ "from": source.display().to_string(), "to": destination.display().to_string(), "files": copied }),
        &format!("pinned {} files into {}\n", copied, destination.display()),
    );

    Ok(())
}

pub fn show(
    context: &Context,
    path: &std::path::Path,
    requests: bool,
    scripts: bool,
    probes: bool,
) -> Result<()> {
    let bundle = CaptureBundle::read(path)?;

    if requests {
        let mut table = Table::new(&["status", "method", "type", "url", "bytes"]);
        for request in &bundle.requests {
            table.push(vec![
                request.status.map(|value| value.to_string()).unwrap_or_default(),
                request.method.clone(),
                request.resource_type.clone().unwrap_or_default(),
                request.url.clone(),
                request.response_body.size.to_string(),
            ]);
        }
        context.emit(&json!(bundle.requests), &table.render());
        return Ok(());
    }

    if scripts {
        let mut table = Table::new(&["bytes", "rewritten", "url"]);
        for script in &bundle.scripts {
            table.push(vec![
                script.body.size.to_string(),
                if script.rewritten { "yes".into() } else { String::new() },
                script.url.clone(),
            ]);
        }
        context.emit(&json!(bundle.scripts), &table.render());
        return Ok(());
    }

    if probes {
        context.emit(
            &json!(bundle.probes),
            &serde_json::to_string_pretty(&bundle.probes).unwrap_or_default(),
        );
        return Ok(());
    }

    let posts = bundle.posts();
    let largest = bundle.largest_script();

    let record = json!({
        "id": bundle.id,
        "target": bundle.target,
        "url": bundle.url,
        "capturedAt": bundle.captured_at.to_rfc3339(),
        "requests": bundle.requests.len(),
        "posts": posts.len(),
        "scripts": bundle.scripts.len(),
        "largestScript": largest.map(|script| json!({ "url": script.url, "bytes": script.body.size })),
        "cookies": bundle.cookies.iter().map(|cookie| cookie.name.clone()).collect::<Vec<_>>(),
        "storage": bundle.storage.len(),
        "console": bundle.console.len(),
        "exceptions": bundle.exceptions.len(),
        "probes": bundle.probes.keys().collect::<Vec<_>>(),
    });

    let plain = format!(
        "{} {}\n  captured {}\n  {} requests ({} posts), {} scripts, {} cookies, {} storage entries\n  {} console lines, {} exceptions\n  largest script: {}\n",
        bundle.target,
        bundle.url,
        bundle.captured_at.to_rfc3339(),
        bundle.requests.len(),
        posts.len(),
        bundle.scripts.len(),
        bundle.cookies.len(),
        bundle.storage.len(),
        bundle.console.len(),
        bundle.exceptions.len(),
        largest
            .map(|script| format!("{} ({} bytes)", script.url, script.body.size))
            .unwrap_or_else(|| "none".to_string())
    );

    context.emit(&record, &plain);
    Ok(())
}
