use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use wre_core::store::Store;
use wre_net::proxy::ProxySpec;

pub use wre_net::emulate::{Fingerprint, Platform, Profile};
pub use wre_net::http::{FetchRequest, FetchResponse};
pub use wre_net::proxy::ProxyScheme;

use wre_net::http::{Client as HttpClient, ClientOptions};

use crate::diag::Recorder;
use crate::error::{ClientError, ClientResult};

pub trait EventSink: Send + Sync {
    fn emit(&self, id: u64, event: &str, data: Value);
}

pub struct DroppedEvents;

impl EventSink for DroppedEvents {
    fn emit(&self, _id: u64, _event: &str, _data: Value) {}
}

pub trait MetricSink: Send + Sync {
    fn record(&self, name: &str, value: f64);
}

#[derive(Default)]
pub struct Counters {
    values: Mutex<HashMap<String, f64>>,
}

impl Counters {
    pub fn snapshot(&self) -> Value {
        let values = self.values.lock().unwrap_or_else(|error| error.into_inner());
        let mut out = serde_json::Map::new();
        for (name, value) in values.iter() {
            out.insert(name.clone(), json!(value));
        }
        Value::Object(out)
    }
}

impl MetricSink for Counters {
    fn record(&self, name: &str, value: f64) {
        let mut values = self.values.lock().unwrap_or_else(|error| error.into_inner());
        *values.entry(name.to_string()).or_insert(0.0) += value;
    }
}

pub struct Http {
    client: HttpClient,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl Http {
    pub fn fetch(&self, request: FetchRequest) -> ClientResult<FetchResponse> {
        let url = request.url.clone();
        self.runtime
            .block_on(self.client.fetch(request))
            .map_err(|error| ClientError::resource(format!("{url}: {error}")))
    }

    pub fn get_text(&self, url: &str) -> ClientResult<String> {
        self.runtime
            .block_on(self.client.get_text(url))
            .map_err(|error| ClientError::resource(format!("{url}: {error}")))
    }

    pub fn get_bytes(&self, url: &str) -> ClientResult<Vec<u8>> {
        self.runtime
            .block_on(self.client.get_bytes(url))
            .map_err(|error| ClientError::resource(format!("{url}: {error}")))
    }

    pub fn raw(&self) -> &HttpClient {
        &self.client
    }

    pub fn fingerprint(&self) -> Fingerprint {
        self.client.fingerprint()
    }

    pub fn user_agent(&self) -> Option<String> {
        self.client.user_agent()
    }
}

pub struct Services {
    runtime: Arc<tokio::runtime::Runtime>,
    clients: Mutex<HashMap<String, HttpClient>>,
    workspace: Option<PathBuf>,
    state_root: PathBuf,
    metrics: Arc<dyn MetricSink>,
}

impl Services {
    pub fn new(
        workspace: Option<PathBuf>,
        state_root: PathBuf,
        metrics: Arc<dyn MetricSink>,
    ) -> ClientResult<Arc<Self>> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|error| {
                ClientError::resource(format!("http runtime did not start: {error}"))
            })?;

        Ok(Arc::new(Self {
            runtime: Arc::new(runtime),
            clients: Mutex::new(HashMap::new()),
            workspace,
            state_root,
            metrics,
        }))
    }

    pub fn detached() -> ClientResult<Arc<Self>> {
        let root = std::env::temp_dir().join("wre-client-state");
        Self::new(None, root, Arc::new(Counters::default()))
    }

    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    pub fn workspace_root(&self) -> Option<&Path> {
        self.workspace.as_deref()
    }

    fn http(&self, options: HttpOptions) -> ClientResult<Http> {
        let key = options.key();

        {
            let clients = self.clients.lock().unwrap_or_else(|error| error.into_inner());
            if let Some(found) = clients.get(&key) {
                return Ok(Http { client: found.clone(), runtime: Arc::clone(&self.runtime) });
            }
        }

        let proxy = match &options.proxy {
            Some(text) => Some(
                ProxySpec::parse(text)
                    .map_err(|error| ClientError::bad_input(format!("proxy rejected: {error}")))?,
            ),
            None => None,
        };

        let mut built = ClientOptions { proxy, ..ClientOptions::default() };
        if let Some(spec) = &options.fingerprint {
            built.fingerprint = Some(spec.parse::<Fingerprint>().map_err(|error| {
                ClientError::bad_input(format!("fingerprint rejected: {error}"))
            })?);
        }
        if let Some(agent) = &options.user_agent {
            built.user_agent = Some(agent.clone());
        }
        if let Some(seconds) = options.timeout_secs {
            built.timeout = Duration::from_secs(seconds);
        }
        built.http2_only = options.http2_only;
        built.redirects = options.redirects;

        let client = HttpClient::new(built)
            .map_err(|error| ClientError::resource(format!("http client failed: {error}")))?;

        let mut clients = self.clients.lock().unwrap_or_else(|error| error.into_inner());
        clients.insert(key, client.clone());

        Ok(Http { client, runtime: Arc::clone(&self.runtime) })
    }
}

#[derive(Debug, Clone, Default)]
pub struct HttpOptions {
    pub proxy: Option<String>,
    pub fingerprint: Option<String>,
    pub user_agent: Option<String>,
    pub timeout_secs: Option<u64>,
    pub http2_only: bool,
    pub redirects: usize,
}

impl HttpOptions {
    pub fn with_proxy(proxy: Option<&str>) -> Self {
        Self { proxy: proxy.map(str::to_string), redirects: 10, ..Self::default() }
    }

    pub fn emulating(mut self, fingerprint: &str) -> Self {
        self.fingerprint = Some(fingerprint.to_string());
        self
    }

    fn key(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.proxy.as_deref().unwrap_or("direct"),
            self.fingerprint.as_deref().unwrap_or("auto"),
            self.user_agent.as_deref().unwrap_or("default"),
            self.timeout_secs.unwrap_or(30),
            self.http2_only,
            self.redirects
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Clock {
    System,
    Fixed(u64),
}

pub struct Ctx {
    pub target: String,
    pub session: String,
    services: Arc<Services>,
    clock: Clock,
    seed: AtomicU64,
    seeded: bool,
    recorder: Arc<Recorder>,
}

impl Ctx {
    pub fn new(
        target: impl Into<String>,
        session: impl Into<String>,
        services: Arc<Services>,
    ) -> Self {
        Self {
            target: target.into(),
            session: session.into(),
            services,
            clock: Clock::System,
            seed: AtomicU64::new(fresh_seed()),
            seeded: false,
            recorder: Arc::new(Recorder::disabled()),
        }
    }

    pub fn with_recorder(mut self, recorder: Arc<Recorder>) -> Self {
        self.recorder = recorder;
        self
    }

    pub fn diag(&self) -> &Arc<Recorder> {
        &self.recorder
    }

    pub fn fact(&self, key: &str, value: Value) {
        self.recorder.fact(key, value);
    }

    pub fn note(&self, level: &str, kind: &str, message: &str, data: Value) {
        self.recorder.record(level, kind, message, data);
    }

    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = AtomicU64::new(seed);
        self.seeded = true;
        self
    }

    pub fn deterministic(&self) -> bool {
        self.seeded && matches!(self.clock, Clock::Fixed(_))
    }

    pub fn now_ms(&self) -> u64 {
        match self.clock {
            Clock::Fixed(value) => value,
            Clock::System => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|value| value.as_millis() as u64)
                .unwrap_or_default(),
        }
    }

    pub fn random_u64(&self) -> u64 {
        let previous = self.seed.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);
        splitmix64(previous.wrapping_add(0x9e37_79b9_7f4a_7c15))
    }

    pub fn random_hex(&self, bytes: usize) -> String {
        let mut out = String::with_capacity(bytes * 2);
        let mut left = bytes;
        while left > 0 {
            let chunk = self.random_u64().to_be_bytes();
            let take = left.min(8);
            for byte in &chunk[..take] {
                out.push_str(&format!("{byte:02x}"));
            }
            left -= take;
        }
        out
    }

    pub fn http(&self, proxy: Option<&str>) -> ClientResult<Http> {
        self.services.http(HttpOptions::with_proxy(proxy))
    }

    pub fn http_with(&self, options: HttpOptions) -> ClientResult<Http> {
        self.services.http(options)
    }

    pub fn workspace(&self) -> Option<&Path> {
        self.services.workspace.as_deref()
    }

    pub fn store(&self) -> ClientResult<Store> {
        let root = self.services.state_root.join(wre_core::paths::safe_name(&self.target));
        let store = Store::new(root);
        store.ensure()?;
        Ok(store)
    }

    pub fn session_store(&self) -> ClientResult<Store> {
        let root = self
            .services
            .state_root
            .join(wre_core::paths::safe_name(&self.target))
            .join(wre_core::paths::safe_name(&self.session));
        let store = Store::new(root);
        store.ensure()?;
        Ok(store)
    }

    pub fn metric(&self, name: &str, value: f64) {
        self.services.metrics.record(name, value);
    }

    pub fn count(&self, name: &str) {
        self.services.metrics.record(name, 1.0);
    }
}

fn splitmix64(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn fresh_seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos() as u64)
        .unwrap_or(0x2545_f491_4f6c_dd1d);
    splitmix64(nanos ^ (std::process::id() as u64).rotate_left(32))
}

pub struct Call {
    pub id: u64,
    pub op: String,
    pub deadline: Option<Instant>,
    cancel: Arc<AtomicBool>,
    events: Arc<dyn EventSink>,
    input: Vec<u8>,
    output: Mutex<Vec<u8>>,
    recorder: Arc<Recorder>,
}

impl Call {
    pub fn new(
        id: u64,
        op: impl Into<String>,
        deadline: Option<Instant>,
        cancel: Arc<AtomicBool>,
        events: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            id,
            op: op.into(),
            deadline,
            cancel,
            events,
            input: Vec::new(),
            output: Mutex::new(Vec::new()),
            recorder: Arc::new(Recorder::disabled()),
        }
    }

    pub fn detached(op: impl Into<String>) -> Self {
        Self {
            id: 0,
            op: op.into(),
            deadline: None,
            cancel: Arc::new(AtomicBool::new(false)),
            events: Arc::new(DroppedEvents),
            input: Vec::new(),
            output: Mutex::new(Vec::new()),
            recorder: Arc::new(Recorder::disabled()),
        }
    }

    pub fn with_recorder(mut self, recorder: Arc<Recorder>) -> Self {
        self.recorder = recorder;
        self
    }

    pub fn diag(&self) -> &Arc<Recorder> {
        &self.recorder
    }

    pub fn debug(&self, kind: &str, data: Value) {
        self.recorder.breadcrumb(&self.op, self.id, kind, data);
    }

    pub fn fact(&self, key: &str, value: Value) {
        self.recorder.fact(key, value);
    }

    pub fn with_input(mut self, bytes: Vec<u8>) -> Self {
        self.input = bytes;
        self
    }

    pub fn input(&self) -> &[u8] {
        &self.input
    }

    pub fn set_output(&self, bytes: Vec<u8>) {
        let mut slot = self.output.lock().unwrap_or_else(|error| error.into_inner());
        *slot = bytes;
    }

    pub fn take_output(&self) -> Vec<u8> {
        let mut slot = self.output.lock().unwrap_or_else(|error| error.into_inner());
        std::mem::take(&mut *slot)
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn expired(&self) -> bool {
        self.deadline.is_some_and(|deadline| Instant::now() >= deadline)
    }

    pub fn remaining(&self) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    pub fn check(&self) -> ClientResult<()> {
        if self.cancelled() {
            return Err(ClientError::cancelled(format!("{} was cancelled", self.op)));
        }
        if self.expired() {
            return Err(ClientError::timeout(format!("{} ran past its deadline", self.op)));
        }
        Ok(())
    }

    pub fn emit(&self, event: &str, data: Value) {
        self.events.emit(self.id, event, data);
    }

    pub fn log(&self, level: &str, text: &str) {
        self.recorder.record(level, "log", text, json!({ "op": self.op }));
        self.events.emit(self.id, "log", json!({ "level": level, "text": text }));
    }

    pub fn progress(&self, done: u64, total: u64, note: &str) {
        self.events
            .emit(self.id, "progress", json!({ "done": done, "total": total, "note": note }));
    }
}
