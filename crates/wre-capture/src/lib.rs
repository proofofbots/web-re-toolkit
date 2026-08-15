pub mod collector;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use wre_cdp::chrome::{Chrome, LaunchOptions};
use wre_cdp::emulation::{self, DeviceProfile};
use wre_cdp::intercept::{Decision, Handler, InterceptOptions, PausedRequest};
use wre_cdp::session::Session;
use wre_core::bundle::{
    BodyRef, BrowserInfo, CaptureBundle, CookieRecord, DocumentRecord, EmulationEntry,
    ScriptRecord, StorageRecord, ToolInfo,
};
use wre_core::error::{Error, Result};
use wre_core::paths::safe_name;
use wre_probe::SurfaceSpec;

use collector::Collected;

#[derive(Clone)]
pub struct CaptureOptions {
    pub target: String,
    pub url: String,
    pub label: Option<String>,
    pub out_dir: PathBuf,
    pub port: u16,
    pub headless: bool,
    pub profile_dir: PathBuf,
    pub wait: Duration,
    pub keep_storage: bool,
    pub device: DeviceProfile,
    pub emulation: Vec<EmulationEntry>,
    pub inject: Vec<String>,
    pub probes: Vec<(String, SurfaceSpec)>,
    pub intercept: Option<InterceptOptions>,
    pub rewrite: Option<Handler>,
    pub script_filter: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    pub body_filter: Arc<dyn Fn(&str, Option<&str>) -> bool + Send + Sync>,
    pub proxy: Option<String>,
    pub origins: Vec<String>,
    pub close_page: bool,
}

impl CaptureOptions {
    pub fn new(target: &str, url: &str, out_dir: impl Into<PathBuf>) -> Self {
        Self {
            target: target.to_string(),
            url: url.to_string(),
            label: None,
            out_dir: out_dir.into(),
            port: wre_cdp::chrome::DEFAULT_PORT,
            headless: false,
            profile_dir: std::env::temp_dir().join("wre-chrome"),
            wait: Duration::from_secs(12),
            keep_storage: false,
            device: DeviceProfile::default(),
            emulation: Vec::new(),
            inject: Vec::new(),
            probes: Vec::new(),
            intercept: None,
            rewrite: None,
            script_filter: Arc::new(|_| true),
            body_filter: Arc::new(|_, kind| {
                matches!(kind, Some("Document") | Some("XHR") | Some("Fetch"))
            }),
            proxy: None,
            origins: Vec::new(),
            close_page: false,
        }
    }

    pub fn with_probe(mut self, name: &str, spec: SurfaceSpec) -> Self {
        self.probes.push((name.to_string(), spec));
        self
    }

    pub fn with_script_rewrite<F>(mut self, rewrite: F) -> Self
    where
        F: Fn(&str, &str) -> Option<String> + Send + Sync + 'static,
    {
        self.intercept = Some(self.intercept.unwrap_or_default());
        self.rewrite = Some(wre_cdp::intercept::rewrite_scripts(rewrite));
        self
    }

    pub fn recording_scripts(mut self) -> Self {
        self.intercept = Some(InterceptOptions::default());
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct CapturedScripts {
    pub entries: Vec<ScriptRecord>,
}

pub async fn run(options: CaptureOptions) -> Result<CaptureBundle> {
    std::fs::create_dir_all(&options.out_dir)
        .map_err(wre_core::error::io(&options.out_dir))?;

    let chrome = Chrome::launch(LaunchOptions {
        port: options.port,
        headless: options.headless,
        profile: options.profile_dir.clone(),
        proxy: options.proxy.clone(),
        window: (options.device.screen_width, options.device.screen_height),
        ..LaunchOptions::default()
    })
    .await?;

    let session = chrome.reuse_page().await?;

    let mut bundle = CaptureBundle::new(&options.target, &options.url);
    bundle.label = options.label.clone();
    bundle.tool = ToolInfo {
        name: "wre-capture".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        command: vec![options.url.clone()],
    };
    bundle.browser = BrowserInfo {
        product: chrome.version.browser.clone(),
        user_agent: chrome.version.user_agent.clone(),
        protocol_version: chrome.version.protocol_version.clone(),
        headless: options.headless,
        profile: Some(options.profile_dir.display().to_string()),
    };

    let mut events = session.events().await;
    let mut collected = Collected::default();

    session.set_buffer_sizes().await?;
    session.enable(&["Page", "Runtime"]).await?;
    session.set_cache_disabled(true).await.ok();

    if !options.keep_storage {
        let mut origins = options.origins.clone();
        if let Ok(parsed) = url::Url::parse(&options.url) {
            origins.push(parsed.origin().ascii_serialization());
        }
        session.clear_browser_state(&origins).await?;
    }

    let mut applied = options.device.entries();
    applied.extend(options.emulation.clone());
    let failures = emulation::apply(&session, &applied).await;
    bundle.emulation = applied;

    if !failures.is_empty() {
        bundle
            .notes
            .insert("emulation-failures".to_string(), json!(failures));
    }

    for source in &options.inject {
        session.add_init_script(source).await?;
    }

    for (_, spec) in &options.probes {
        session.add_init_script(&spec.build()?).await?;
    }

    let scripts: Arc<Mutex<Vec<ScriptRecord>>> = Arc::new(Mutex::new(Vec::new()));

    let interceptor = match &options.intercept {
        Some(config) => {
            let handler = build_handler(&options, Arc::clone(&scripts));
            Some(
                wre_cdp::intercept::start(&session, config.clone(), handler)
                    .await?,
            )
        }
        None => None,
    };

    session.navigate(&options.url).await?;

    let deadline = tokio::time::Instant::now() + options.wait;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        tokio::select! {
            event = events.recv() => {
                match event {
                    Some(event) => collected.absorb(&event),
                    None => break,
                }
            }
            _ = tokio::time::sleep(remaining.min(Duration::from_millis(200))) => {}
        }
    }

    collector::drain(&mut events, &mut collected).await;

    for (name, spec) in &options.probes {
        if let Ok(value) = session.evaluate_json(&spec.dump_expression()).await {
            if !value.is_null() {
                bundle.probes.insert(name.clone(), value);
            }
        }
    }

    let page_state = session
        .evaluate_json(
            r#"({
                href: location.href,
                title: document.title,
                cookies: document.cookie,
                storage: (function () {
                    var out = [];
                    try {
                        for (var i = 0; i < localStorage.length; i++) {
                            var key = localStorage.key(i);
                            out.push({ kind: "local", key: key, value: String(localStorage.getItem(key)).slice(0, 2000) });
                        }
                    } catch (error) {}
                    try {
                        for (var j = 0; j < sessionStorage.length; j++) {
                            var skey = sessionStorage.key(j);
                            out.push({ kind: "session", key: skey, value: String(sessionStorage.getItem(skey)).slice(0, 2000) });
                        }
                    } catch (error) {}
                    return out;
                })()
            })"#,
        )
        .await
        .unwrap_or(Value::Null);

    let origin = url::Url::parse(&options.url)
        .map(|parsed| parsed.origin().ascii_serialization())
        .unwrap_or_default();

    if let Some(list) = page_state.get("storage").and_then(Value::as_array) {
        for entry in list {
            bundle.storage.push(StorageRecord {
                origin: origin.clone(),
                kind: entry
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("local")
                    .to_string(),
                key: entry
                    .get("key")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                value: entry
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }

    let html = session.document_html().await.unwrap_or_default();
    if !html.is_empty() {
        let body = BodyRef::store(
            &options.out_dir,
            &format!("{}.document.html", safe_name(&options.target)),
            html.as_bytes(),
            true,
        )?;

        bundle.documents.push(DocumentRecord {
            url: page_state
                .get("href")
                .and_then(Value::as_str)
                .unwrap_or(&options.url)
                .to_string(),
            frame: None,
            body,
            title: page_state
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }

    for cookie in session.cookies().await.unwrap_or_default() {
        bundle.cookies.push(CookieRecord {
            name: cookie
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            value: cookie
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            domain: cookie
                .get("domain")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            path: cookie
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            expires: cookie.get("expires").and_then(Value::as_f64),
            http_only: cookie
                .get("httpOnly")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            secure: cookie.get("secure").and_then(Value::as_bool).unwrap_or(false),
            same_site: cookie
                .get("sameSite")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }

    let (mut requests, console, exceptions) = collected.finish();

    let filter = Arc::clone(&options.body_filter);
    collector::fetch_bodies(&session, &mut requests, &options.out_dir, move |request| {
        filter(&request.url, request.resource_type.as_deref())
    })
    .await
    .ok();

    bundle.requests = requests;
    bundle.console = console;
    bundle.exceptions = exceptions;
    bundle.scripts = scripts.lock().map(|guard| guard.clone()).unwrap_or_default();

    if let Some(task) = interceptor {
        task.abort();
    }

    if options.close_page {
        chrome.close_target(&session.target_id).await.ok();
    } else {
        session.navigate("about:blank").await.ok();
    }

    bundle.write(&options.out_dir)?;
    Ok(bundle)
}

fn build_handler(options: &CaptureOptions, scripts: Arc<Mutex<Vec<ScriptRecord>>>) -> Handler {
    let out_dir = options.out_dir.clone();
    let script_filter = Arc::clone(&options.script_filter);
    let rewrite = options.rewrite.clone();

    Arc::new(move |request: &PausedRequest| {
        if !request.is_response_stage() {
            return Decision::Continue;
        }

        let Some(bytes) = request.body.clone() else {
            return Decision::Continue;
        };

        if !script_filter(&request.url) {
            return Decision::Continue;
        }

        let text_like = std::str::from_utf8(&bytes).is_ok();
        let name = format!("{}.js", safe_name(&request.url));

        let stored = BodyRef::store(&out_dir, &name, &bytes, text_like).unwrap_or_default();

        let decision = match &rewrite {
            Some(handler) => handler(request),
            None => Decision::Continue,
        };

        let served = match &decision {
            Decision::Fulfill { body, .. } => {
                let served_name = format!("{}.served.js", safe_name(&request.url));
                BodyRef::store(&out_dir, &served_name, body, true).ok()
            }
            _ => None,
        };

        if let Ok(mut guard) = scripts.lock() {
            guard.push(ScriptRecord {
                url: request.url.clone(),
                status: request.status,
                body: stored,
                rewritten: served.is_some(),
                served,
                tags: vec![request.resource_type.clone()],
            });
        }

        decision
    })
}

pub async fn probe_dump(session: &Session, spec: &SurfaceSpec) -> Result<Value> {
    session.evaluate_json(&spec.dump_expression()).await
}

pub fn pin(bundle: &CaptureBundle, from: &std::path::Path, to: &std::path::Path) -> Result<usize> {
    if !from.exists() {
        return Err(Error::msg(format!(
            "capture directory {} does not exist",
            from.display()
        )));
    }

    let copied = wre_core::store::copy_tree(from, to)?;
    bundle.write(to)?;
    Ok(copied)
}
