use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::error::{ClientError, ClientResult};

pub const REPORT_SCHEMA: u32 = 1;
pub const REPORT_SUFFIX: &str = ".diag.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagMode {
    Off,
    #[default]
    OnError,
    Always,
}

impl DiagMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "off" | "none" | "0" | "false" => Some(DiagMode::Off),
            "on_error" | "onerror" | "error" | "1" | "true" => Some(DiagMode::OnError),
            "always" | "all" => Some(DiagMode::Always),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            DiagMode::Off => "off",
            DiagMode::OnError => "on_error",
            DiagMode::Always => "always",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagConfig {
    pub mode: DiagMode,
    pub dir: Option<PathBuf>,
    pub max_events: usize,
    pub max_value_bytes: usize,
    pub include_params: bool,
    pub keep_files: usize,
}

impl Default for DiagConfig {
    fn default() -> Self {
        Self {
            mode: DiagMode::OnError,
            dir: None,
            max_events: 400,
            max_value_bytes: 4096,
            include_params: false,
            keep_files: 20,
        }
    }
}

impl DiagConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(mode) = std::env::var("WRE_DIAG") {
            if let Some(parsed) = DiagMode::parse(&mode) {
                config.mode = parsed;
            }
        }

        if let Ok(dir) = std::env::var("WRE_DIAG_DIR") {
            if !dir.trim().is_empty() {
                config.dir = Some(PathBuf::from(dir));
            }
        }

        if let Ok(value) = std::env::var("WRE_DIAG_PARAMS") {
            config.include_params = matches!(value.trim(), "1" | "true" | "yes");
        }

        config
    }

    pub fn merge(&mut self, value: &Value) {
        let Some(entries) = value.as_object() else {
            return;
        };

        if let Some(mode) = entries.get("mode").and_then(Value::as_str) {
            if let Some(parsed) = DiagMode::parse(mode) {
                self.mode = parsed;
            }
        }

        if let Some(dir) = entries.get("dir").and_then(Value::as_str) {
            if !dir.trim().is_empty() {
                self.dir = Some(PathBuf::from(dir));
            }
        }

        if let Some(value) = entries.get("max_events").and_then(Value::as_u64) {
            self.max_events = value.clamp(10, 20_000) as usize;
        }

        if let Some(value) = entries.get("max_value_bytes").and_then(Value::as_u64) {
            self.max_value_bytes = value.clamp(256, 1_048_576) as usize;
        }

        if let Some(value) = entries.get("include_params").and_then(Value::as_bool) {
            self.include_params = value;
        }

        if let Some(value) = entries.get("keep_files").and_then(Value::as_u64) {
            self.keep_files = value.clamp(1, 500) as usize;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub at_ms: u64,
    pub level: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub schema: u32,
    pub kind: String,
    pub generated_at: String,
    pub reason: String,
    pub host: Value,
    pub target: String,
    pub client_version: String,
    pub session: String,
    pub session_ms: u64,
    pub capabilities: Value,
    pub config: Value,
    pub environment: Value,
    pub calls: Value,
    pub facts: Value,
    pub client: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<ClientError>,
    pub dropped_events: u64,
    pub events: Vec<Event>,
}

pub struct Recorder {
    config: DiagConfig,
    target: String,
    session: String,
    client_version: Mutex<String>,
    started: Instant,
    opened_at: chrono::DateTime<chrono::Utc>,
    events: Mutex<VecDeque<Event>>,
    facts: Mutex<Map<String, Value>>,
    host: Mutex<Value>,
    capabilities: Mutex<Value>,
    config_snapshot: Mutex<Value>,
    dropped: AtomicU64,
    calls: AtomicU64,
    failures: AtomicU64,
    last_report: Mutex<Option<PathBuf>>,
}

impl Recorder {
    pub fn new(config: DiagConfig, target: impl Into<String>, session: impl Into<String>) -> Self {
        Self {
            config,
            target: target.into(),
            session: session.into(),
            client_version: Mutex::new(String::new()),
            started: Instant::now(),
            opened_at: chrono::Utc::now(),
            events: Mutex::new(VecDeque::new()),
            facts: Mutex::new(Map::new()),
            host: Mutex::new(Value::Null),
            capabilities: Mutex::new(Value::Null),
            config_snapshot: Mutex::new(Value::Null),
            dropped: AtomicU64::new(0),
            calls: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            last_report: Mutex::new(None),
        }
    }

    pub fn disabled() -> Self {
        Self::new(
            DiagConfig { mode: DiagMode::Off, ..DiagConfig::default() },
            "unknown",
            "detached",
        )
    }

    pub fn mode(&self) -> DiagMode {
        self.config.mode
    }

    pub fn enabled(&self) -> bool {
        self.config.mode != DiagMode::Off
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn session(&self) -> &str {
        &self.session
    }

    pub fn set_host(&self, value: Value) {
        let mut slot = self.host.lock().unwrap_or_else(|error| error.into_inner());
        *slot = value;
    }

    pub fn set_capabilities(&self, value: Value) {
        let mut slot = self.capabilities.lock().unwrap_or_else(|error| error.into_inner());
        *slot = value;
    }

    pub fn set_client_version(&self, value: impl Into<String>) {
        let mut slot = self.client_version.lock().unwrap_or_else(|error| error.into_inner());
        *slot = value.into();
    }

    pub fn set_config(&self, value: &Value) {
        let scrubbed = scrub(value, self.config.max_value_bytes);
        let mut slot = self.config_snapshot.lock().unwrap_or_else(|error| error.into_inner());
        *slot = scrubbed;
    }

    pub fn fact(&self, key: &str, value: Value) {
        if !self.enabled() {
            return;
        }
        let mut facts = self.facts.lock().unwrap_or_else(|error| error.into_inner());
        facts.insert(key.to_string(), scrub(&value, self.config.max_value_bytes));
    }

    pub fn record(&self, level: &str, kind: &str, message: &str, data: Value) {
        self.push(Event {
            at_ms: self.elapsed_ms(),
            level: level.to_string(),
            kind: kind.to_string(),
            op: None,
            id: None,
            message: message.to_string(),
            data: scrub(&data, self.config.max_value_bytes),
        });
    }

    pub fn breadcrumb(&self, op: &str, id: u64, kind: &str, data: Value) {
        self.push(Event {
            at_ms: self.elapsed_ms(),
            level: "debug".to_string(),
            kind: kind.to_string(),
            op: Some(op.to_string()),
            id: Some(id),
            message: String::new(),
            data: scrub(&data, self.config.max_value_bytes),
        });
    }

    pub fn op_started(&self, id: u64, op: &str, params: &Value) {
        self.calls.fetch_add(1, Ordering::Relaxed);

        if !self.enabled() {
            return;
        }

        let data = if self.config.include_params {
            scrub(params, self.config.max_value_bytes)
        } else {
            outline(params)
        };

        self.push(Event {
            at_ms: self.elapsed_ms(),
            level: "info".to_string(),
            kind: "call.start".to_string(),
            op: Some(op.to_string()),
            id: Some(id),
            message: String::new(),
            data,
        });
    }

    pub fn op_finished(
        &self,
        id: u64,
        op: &str,
        took_ms: u64,
        outcome: Result<&Value, &ClientError>,
    ) {
        if let Err(error) = outcome {
            self.failures.fetch_add(1, Ordering::Relaxed);
            self.push(Event {
                at_ms: self.elapsed_ms(),
                level: "error".to_string(),
                kind: "call.failed".to_string(),
                op: Some(op.to_string()),
                id: Some(id),
                message: error.message.clone(),
                data: json!({
                    "kind": error.kind.as_str(),
                    "retryable": error.retryable,
                    "took_ms": took_ms,
                    "detail": scrub(&error.detail, self.config.max_value_bytes),
                }),
            });
            return;
        }

        if !self.enabled() {
            return;
        }

        let result = outcome.unwrap_or(&Value::Null);
        let data = if self.config.include_params {
            json!({ "took_ms": took_ms, "result": scrub(result, self.config.max_value_bytes) })
        } else {
            json!({ "took_ms": took_ms, "result": outline(result) })
        };

        self.push(Event {
            at_ms: self.elapsed_ms(),
            level: "info".to_string(),
            kind: "call.done".to_string(),
            op: Some(op.to_string()),
            id: Some(id),
            message: String::new(),
            data,
        });
    }

    pub fn should_write(&self, failed: bool) -> bool {
        match self.config.mode {
            DiagMode::Off => false,
            DiagMode::OnError => failed,
            DiagMode::Always => true,
        }
    }

    pub fn report(&self, reason: &str, failure: Option<&ClientError>, client: Value) -> Report {
        let events = {
            let events = self.events.lock().unwrap_or_else(|error| error.into_inner());
            events.iter().cloned().collect()
        };

        Report {
            schema: REPORT_SCHEMA,
            kind: "wre-diagnostics".to_string(),
            generated_at: chrono::Utc::now().to_rfc3339(),
            reason: reason.to_string(),
            host: self.host.lock().unwrap_or_else(|error| error.into_inner()).clone(),
            target: self.target.clone(),
            client_version: self
                .client_version
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
            session: self.session.clone(),
            session_ms: self.elapsed_ms(),
            capabilities: self
                .capabilities
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
            config: self
                .config_snapshot
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
            environment: environment(self.opened_at),
            calls: json!({
                "total": self.calls.load(Ordering::Relaxed),
                "failed": self.failures.load(Ordering::Relaxed),
            }),
            facts: Value::Object(
                self.facts.lock().unwrap_or_else(|error| error.into_inner()).clone(),
            ),
            client: scrub(&client, self.config.max_value_bytes),
            failure: failure.cloned(),
            dropped_events: self.dropped.load(Ordering::Relaxed),
            events,
        }
    }

    pub fn write(&self, report: &Report, root: &Path) -> ClientResult<PathBuf> {
        let dir = self.config.dir.clone().unwrap_or_else(|| root.to_path_buf());
        std::fs::create_dir_all(&dir).map_err(|error| {
            ClientError::resource(format!(
                "diagnostics directory {} could not be made: {error}",
                dir.display()
            ))
        })?;

        let name = format!(
            "{}-{}-{}{REPORT_SUFFIX}",
            wre_core::paths::safe_name(&self.target),
            wre_core::paths::stamp(),
            wre_core::paths::safe_name(&self.session)
        );

        let path = dir.join(name);
        let text = serde_json::to_string_pretty(report)
            .map_err(|error| ClientError::internal(format!("report did not serialise: {error}")))?;

        std::fs::write(&path, text).map_err(|error| {
            ClientError::resource(format!("report {} could not be written: {error}", path.display()))
        })?;

        prune(&dir, self.config.keep_files);

        let mut slot = self.last_report.lock().unwrap_or_else(|error| error.into_inner());
        *slot = Some(path.clone());

        Ok(path)
    }

    pub fn last_report(&self) -> Option<PathBuf> {
        self.last_report
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn push(&self, event: Event) {
        if !self.enabled() {
            return;
        }

        let mut events = self.events.lock().unwrap_or_else(|error| error.into_inner());
        while events.len() >= self.config.max_events {
            events.pop_front();
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        events.push_back(event);
    }

    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}

fn prune(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let mut reports: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(REPORT_SUFFIX))
        })
        .collect();

    if reports.len() <= keep {
        return;
    }

    reports.sort();
    let extra = reports.len() - keep;
    for path in reports.into_iter().take(extra) {
        let _ = std::fs::remove_file(path);
    }
}

fn environment(opened_at: chrono::DateTime<chrono::Utc>) -> Value {
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "family": std::env::consts::FAMILY,
        "opened_at": opened_at.to_rfc3339(),
        "cpus": std::thread::available_parallelism().map(|value| value.get()).unwrap_or(0),
        "exe": std::env::current_exe()
            .ok()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
    })
}

const SENSITIVE: [&str; 18] = [
    "password",
    "passwd",
    "secret",
    "token",
    "authorization",
    "auth",
    "cookie",
    "cookies",
    "set-cookie",
    "apikey",
    "api_key",
    "key",
    "credential",
    "credentials",
    "proxy",
    "bearer",
    "session_token",
    "private",
];

fn sensitive(key: &str) -> bool {
    let lowered = key.to_lowercase();
    SENSITIVE.iter().any(|marker| lowered == *marker || lowered.ends_with(marker))
}

pub fn scrub(value: &Value, max_bytes: usize) -> Value {
    match value {
        Value::Object(entries) => {
            let mut out = Map::new();
            for (key, item) in entries {
                if sensitive(key) {
                    out.insert(key.clone(), Value::String(redacted(item)));
                } else {
                    out.insert(key.clone(), scrub(item, max_bytes));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| scrub(item, max_bytes)).collect())
        }
        Value::String(text) if text.len() > max_bytes => {
            let head: String = text.chars().take(max_bytes / 2).collect();
            Value::String(format!(
                "{head}[truncated {} bytes, sha256 {}]",
                text.len(),
                wre_core::digest::sha256_short(text.as_bytes())
            ))
        }
        other => other.clone(),
    }
}

fn redacted(value: &Value) -> String {
    match value {
        Value::Null => "redacted:null".to_string(),
        Value::String(text) => format!(
            "redacted:{} bytes:sha256 {}",
            text.len(),
            wre_core::digest::sha256_short(text.as_bytes())
        ),
        other => {
            let text = other.to_string();
            format!(
                "redacted:{} bytes:sha256 {}",
                text.len(),
                wre_core::digest::sha256_short(text.as_bytes())
            )
        }
    }
}

pub fn outline(value: &Value) -> Value {
    match value {
        Value::Null => json!({ "type": "null" }),
        Value::Bool(_) => json!({ "type": "bool" }),
        Value::Number(number) => json!({ "type": "number", "value": number }),
        Value::String(text) => json!({
            "type": "string",
            "bytes": text.len(),
            "sha256": wre_core::digest::sha256_short(text.as_bytes()),
        }),
        Value::Array(items) => json!({ "type": "list", "len": items.len() }),
        Value::Object(entries) => {
            let mut keys: Vec<&String> = entries.keys().collect();
            keys.sort();
            json!({
                "type": "object",
                "keys": keys,
            })
        }
    }
}
