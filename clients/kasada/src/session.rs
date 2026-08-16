use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use wre_client::context::{FetchRequest, FetchResponse, Http, Jar};
use wre_client::error::{ClientError, ClientResult};
use wre_live::realm::RealmOptions;
use wre_sandbox::browser::{Answer, CookieStore, Hooks, Request, Transport, now_ms};
use wre_sandbox::graph::{Graph, GraphPage, GraphProfile, open};

use crate::discover::{Surface, surface};
use crate::report;
use crate::token::{self, Verdict};

const NAVIGATE: [(&str, &str); 7] = [
    (
        "accept",
        "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8",
    ),
    ("accept-language", "en-US,en;q=0.9"),
    ("sec-fetch-dest", "document"),
    ("sec-fetch-mode", "navigate"),
    ("sec-fetch-site", "none"),
    ("sec-fetch-user", "?1"),
    ("upgrade-insecure-requests", "1"),
];

const SCRIPT: [(&str, &str); 5] = [
    ("accept", "*/*"),
    ("accept-language", "en-US,en;q=0.9"),
    ("sec-fetch-dest", "script"),
    ("sec-fetch-mode", "no-cors"),
    ("sec-fetch-site", "same-origin"),
];

pub const REPORT_HOST: &str = "reporting.cdndex.io";

pub fn client_hints(user_agent: &str) -> Vec<(String, String)> {
    let major = user_agent
        .split("Chrome/")
        .nth(1)
        .and_then(|rest| rest.split('.').next())
        .unwrap_or("140")
        .to_string();

    let platform = if user_agent.contains("Windows") {
        "Windows"
    } else if user_agent.contains("Mac OS X") {
        "macOS"
    } else if user_agent.contains("Android") {
        "Android"
    } else if user_agent.contains("Linux") {
        "Linux"
    } else {
        "macOS"
    };

    let mobile = if user_agent.contains("Mobile") {
        "?1"
    } else {
        "?0"
    };

    vec![
        (
            "sec-ch-ua".to_string(),
            format!(
                "\"Not=A?Brand\";v=\"99\", \"Google Chrome\";v=\"{major}\", \"Chromium\";v=\"{major}\""
            ),
        ),
        ("sec-ch-ua-mobile".to_string(), mobile.to_string()),
        ("sec-ch-ua-platform".to_string(), format!("\"{platform}\"")),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub wait_ms: f64,
    pub step_ms: f64,
    pub friction_ms: f64,
    pub timeout_ms: u64,
    pub paced: bool,
    pub report: bool,
    pub version: String,
    pub frames: usize,
    pub capture_vector: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            wait_ms: 20_000.0,
            step_ms: 100.0,
            friction_ms: 0.12,
            timeout_ms: 90_000,
            paced: true,
            report: false,
            version: "j-1.2.661".to_string(),
            frames: 4,
            capture_vector: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sent {
    pub url: String,
    pub method: String,
    pub status: u16,
    pub bytes: usize,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Issued {
    pub token: String,
    pub accepted: bool,
    pub clearance: Option<String>,
    pub server_time: Option<i64>,
    pub received_at: i64,
    pub status: u16,
    pub payload_bytes: usize,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
}

struct Live {
    http: Arc<Http>,
    origin: String,
    referer: String,
    user_agent: String,
    report: bool,
    sent: Mutex<Vec<Sent>>,
    issued: Mutex<Option<Issued>>,
    payload: Mutex<Option<Vec<u8>>>,
    reports: Mutex<Vec<Value>>,
}

impl Live {
    fn absolute(&self, url: &str) -> Option<String> {
        let base = Url::parse(&self.referer).ok()?;
        let parsed = base.join(url).ok()?;

        matches!(parsed.scheme(), "http" | "https").then(|| parsed.to_string())
    }

    fn same_origin(&self, url: &str) -> bool {
        match (Url::parse(url), Url::parse(&self.origin)) {
            (Ok(target), Ok(origin)) => target.host_str() == origin.host_str(),
            _ => false,
        }
    }

    fn note(&self, sent: Sent) {
        let mut list = self.sent.lock().unwrap_or_else(|error| error.into_inner());
        list.push(sent);
    }
}

impl Transport for Live {
    fn send(&self, request: &Request) -> Answer {
        let body = request.bytes();
        let bytes = body.as_ref().map(Vec::len).unwrap_or_default();
        let Some(url) = self.absolute(&request.url) else {
            self.note(Sent {
                url: request.url.clone(),
                method: request.method.to_uppercase(),
                status: 0,
                bytes,
                source: request.source.clone(),
                blocked: Some("not an http url".to_string()),
            });

            return Answer {
                status: 0,
                body: String::new(),
                headers: Vec::new(),
            };
        };

        let submission = url.split('?').next().unwrap_or_default().ends_with("/tl");

        if url.contains(REPORT_HOST) {
            let decoded = body.as_deref().and_then(report::decode);

            if let Some(found) = decoded {
                let mut list = self
                    .reports
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                list.push(found);
            }

            if !self.report {
                self.note(Sent {
                    url: url.clone(),
                    method: request.method.to_uppercase(),
                    status: 200,
                    bytes,
                    source: request.source.clone(),
                    blocked: Some("the self report is held back".to_string()),
                });

                return Answer {
                    status: 200,
                    body: r#"{"": ""}"#.to_string(),
                    headers: Vec::new(),
                };
            }
        }

        if !self.same_origin(&url) && !url.contains(REPORT_HOST) {
            self.note(Sent {
                url: url.clone(),
                method: request.method.to_uppercase(),
                status: 0,
                bytes,
                source: request.source.clone(),
                blocked: Some("cross origin".to_string()),
            });

            return Answer {
                status: 0,
                body: String::new(),
                headers: Vec::new(),
            };
        }

        if submission && let Some(found) = &body {
            let mut slot = self
                .payload
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            *slot = Some(found.clone());
        }

        let mut headers: BTreeMap<String, String> = BTreeMap::from([
            ("accept".to_string(), "*/*".to_string()),
            ("accept-language".to_string(), "en-US,en;q=0.9".to_string()),
            ("origin".to_string(), self.origin.clone()),
            ("referer".to_string(), self.referer.clone()),
            ("sec-fetch-dest".to_string(), "empty".to_string()),
            ("sec-fetch-mode".to_string(), "cors".to_string()),
            ("sec-fetch-site".to_string(), "same-origin".to_string()),
            ("user-agent".to_string(), self.user_agent.clone()),
        ]);

        for (name, value) in client_hints(&self.user_agent) {
            headers.insert(name, value);
        }

        for (name, value) in &request.headers {
            headers.insert(name.to_lowercase(), value.clone());
        }

        let outgoing = FetchRequest {
            url: url.clone(),
            method: request.method.to_uppercase(),
            headers: headers.into_iter().collect(),
            body,
            fingerprint: None,
        };

        let answer = match self.http.fetch(outgoing) {
            Ok(response) => Answer {
                status: response.status,
                body: response.text(),
                headers: response
                    .headers
                    .iter()
                    .filter(|(name, _)| !name.eq_ignore_ascii_case("set-cookie"))
                    .map(|(name, value)| (name.to_lowercase(), value.clone()))
                    .collect(),
            },
            Err(_) => Answer {
                status: 0,
                body: String::new(),
                headers: Vec::new(),
            },
        };

        if submission {
            let token = answer.header("x-kpsdk-ct").unwrap_or_default().to_string();

            if !token.is_empty() {
                let mut slot = self
                    .issued
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                *slot = Some(Issued {
                    token,
                    accepted: answer.header("x-kpsdk-cr") == Some("true"),
                    clearance: answer.header("x-kpsdk-r").map(str::to_string),
                    server_time: answer
                        .header("x-kpsdk-st")
                        .and_then(|value| value.parse().ok()),
                    received_at: now_ms() as i64,
                    status: answer.status,
                    payload_bytes: bytes,
                    headers: crate::token::kasada_headers(&answer.headers),
                });
            }
        }

        self.note(Sent {
            url,
            method: request.method.to_uppercase(),
            status: answer.status,
            bytes,
            source: request.source.clone(),
            blocked: None,
        });

        answer
    }
}

struct Cookies {
    jar: Jar,
    url: String,
}

impl CookieStore for Cookies {
    fn read(&self) -> String {
        self.jar.script_header(&self.url)
    }

    fn write(&self, assignment: &str) {
        let _ = self.jar.add(&self.url, assignment);
    }
}

pub struct Session {
    http: Arc<Http>,
    jar: Jar,
    profile: Option<GraphProfile>,
    profile_id: String,
    settings: Settings,
    user_agent: String,
    page_url: String,
    html: String,
    status: u16,
    surface: Surface,
    agent_url: String,
    agent_bytes: usize,
    agent_source: String,
    loader: bool,
    browser: Option<Graph>,
    transport: Option<Arc<Live>>,
}

impl Session {
    pub fn new(
        http: Arc<Http>,
        jar: Jar,
        profile: Option<GraphProfile>,
        profile_id: String,
        settings: Settings,
        user_agent: String,
    ) -> Self {
        Self {
            http,
            jar,
            profile,
            profile_id,
            settings,
            user_agent,
            page_url: String::new(),
            html: String::new(),
            status: 0,
            surface: Surface::default(),
            agent_url: String::new(),
            agent_bytes: 0,
            agent_source: String::new(),
            loader: false,
            browser: None,
            transport: None,
        }
    }

    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    pub fn page_url(&self) -> &str {
        &self.page_url
    }

    pub fn agent_source(&self) -> &str {
        &self.agent_source
    }

    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    pub fn jar(&self) -> &Jar {
        &self.jar
    }

    pub fn is_open(&self) -> bool {
        self.browser.is_some()
    }

    pub fn version(&self) -> &str {
        self.surface
            .version
            .as_deref()
            .unwrap_or(&self.settings.version)
    }

    pub fn cookie_pairs(&self) -> Vec<(String, String)> {
        let url = if self.page_url.is_empty() {
            "https://localhost/"
        } else {
            &self.page_url
        };

        self.jar
            .matching(url)
            .into_iter()
            .map(|cookie| (cookie.name, cookie.value))
            .collect()
    }

    pub fn fetch(&self, request: FetchRequest) -> ClientResult<FetchResponse> {
        self.http.fetch(request)
    }

    pub fn navigate(&mut self, url: &str) -> ClientResult<FetchResponse> {
        let mut request = FetchRequest::get(url).header("user-agent", self.user_agent.clone());
        for (name, value) in NAVIGATE {
            request = request.header(name, value);
        }
        for (name, value) in client_hints(&self.user_agent) {
            request = request.header(name, value);
        }

        let response = self.fetch(request)?;

        self.page_url = response.url.clone();
        self.html = response.text();
        self.status = response.status;
        self.surface = surface(&self.html, &self.page_url);

        Ok(response)
    }

    pub fn fetch_script(&self, url: &str) -> ClientResult<String> {
        let mut request = FetchRequest::get(url)
            .header("user-agent", self.user_agent.clone())
            .header("referer", self.page_url.clone());

        for (name, value) in SCRIPT {
            request = request.header(name, value);
        }
        for (name, value) in client_hints(&self.user_agent) {
            request = request.header(name, value);
        }

        let response = self.fetch(request)?;

        if response.status != 200 {
            return Err(ClientError::resource(format!(
                "the script at {url} answered {}",
                response.status
            )));
        }

        Ok(response.text())
    }

    fn origin(&self) -> String {
        Url::parse(&self.page_url)
            .ok()
            .map(|parsed| {
                format!(
                    "{}://{}",
                    parsed.scheme(),
                    parsed.host_str().unwrap_or_default()
                )
            })
            .unwrap_or_default()
    }

    pub fn mount(&mut self) -> ClientResult<()> {
        let transport = Arc::new(Live {
            http: Arc::clone(&self.http),
            origin: self.origin(),
            referer: self.page_url.clone(),
            user_agent: self.user_agent.clone(),
            report: self.settings.report,
            sent: Mutex::new(Vec::new()),
            issued: Mutex::new(None),
            payload: Mutex::new(None),
            reports: Mutex::new(Vec::new()),
        });

        let hooks = Hooks {
            transport: Arc::clone(&transport) as Arc<dyn Transport>,
            cookies: Arc::new(Cookies {
                jar: self.jar.clone(),
                url: self.page_url.clone(),
            }),
        };

        let page = GraphPage {
            url: self.page_url.clone(),
            referrer: String::new(),
            entries: self.timing_entries(),
            cookies: self.jar.script_header(&self.page_url),
            frames: self.settings.frames,
            capture_cipher: self.settings.capture_vector,
        };

        let options = RealmOptions {
            timeout: Duration::from_millis(self.settings.timeout_ms.max(1000)),
            timers: false,
            codecs: false,
            clock_ms: None,
            random_seed: None,
            heap_limit_mb: None,
        };

        let profile = self.profile.as_ref().ok_or_else(|| {
            ClientError::resource("no graph profile is loaded, so there is nothing to mount")
        })?;

        let browser = open(profile, &page, hooks, options)
            .map_err(|error| ClientError::internal(format!("the sandbox did not open: {error}")))?;

        self.browser = Some(browser);
        self.transport = Some(transport);
        Ok(())
    }

    fn timing_entries(&self) -> Vec<Value> {
        let mut entries = vec![
            json!({
                "entryType": "navigation",
                "name": self.page_url,
                "startTime": 0.0,
                "fetchStart": 1.2,
                "requestStart": 12.4,
                "responseStart": 96.7,
                "responseEnd": 101.3,
                "domComplete": 143.9,
                "loadEventEnd": 145.1,
                "type": "navigate",
                "transferSize": self.html.len() + 300,
            }),
            json!({ "entryType": "visibility-state", "name": "visible", "startTime": 0.0, "duration": 0.0 }),
        ];

        if !self.agent_url.is_empty() {
            let decoded = self.agent_bytes as f64;
            let encoded = (decoded / 3.7).round();
            let duration = 587.0;

            entries.push(json!({
                "entryType": "resource",
                "name": self.agent_url,
                "initiatorType": "script",
                "startTime": 150.0,
                "duration": duration,
                "fetchStart": 150.0,
                "domainLookupStart": 150.0,
                "domainLookupEnd": 150.0,
                "connectStart": 150.0,
                "connectEnd": 150.0,
                "secureConnectionStart": 150.0,
                "requestStart": 151.0,
                "responseStart": 150.0 + duration * 0.8,
                "responseEnd": 150.0 + duration,
                "transferSize": encoded + 300.0,
                "encodedBodySize": encoded,
                "decodedBodySize": decoded,
                "nextHopProtocol": "h2",
                "renderBlockingStatus": "non-blocking",
                "responseStatus": 200,
            }));
        }

        entries
    }

    fn browser_mut(&mut self) -> ClientResult<&mut Graph> {
        self.browser
            .as_mut()
            .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))
    }

    fn run(&mut self, source: &str, name: &str) -> ClientResult<Value> {
        let browser = self.browser_mut()?;

        match browser.run(source, name, false) {
            Ok(None) => Ok(Value::Null),
            Ok(Some(thrown)) => Ok(thrown),
            Err(error) => Ok(json!(error.to_string())),
        }
    }

    fn preamble(&self) -> Option<String> {
        let start = self.html.find("<script>")? + "<script>".len();
        let end = self.html[start..].find("</script>")? + start;
        let body = self.html[start..end].trim();

        (!body.is_empty()).then(|| body.to_string())
    }

    pub fn open(&mut self, url: &str) -> ClientResult<Value> {
        let page = self.navigate(url)?;

        let Some(agent) = self.surface.script.clone() else {
            return Err(ClientError::unsupported(format!(
                "{} answered {} and named no ips.js, so there is no interrogation to run",
                response_url(&page),
                page.status
            )));
        };

        let source = self.fetch_script(&agent)?;

        if source.len() < 1000 {
            return Err(ClientError::resource(format!(
                "the agent script is only {} bytes, that is not a build",
                source.len()
            )));
        }

        self.agent_url = agent.clone();
        self.agent_bytes = source.len();
        self.agent_source = source.clone();
        self.mount()?;

        let preamble = self.preamble().unwrap_or_else(|| {
            "window.KPSDK={};KPSDK.now=typeof performance!=='undefined'&&performance.now?\
             performance.now.bind(performance):Date.now.bind(Date);KPSDK.start=KPSDK.now();"
                .to_string()
        });

        let url = self.page_url.clone();
        let browser = self.browser_mut()?;
        browser
            .run(&preamble, &url, true)
            .map_err(|error| ClientError::internal(format!("the preamble did not run: {error}")))?;

        let started = Instant::now();
        let threw = self.run(&source, &agent)?;
        let waited = self.settle_until_issued()?;

        let issued = self.issued();

        Ok(json!({
            "page": { "url": self.page_url, "status": self.status, "bytes": self.html.len() },
            "agent": { "url": self.agent_url, "bytes": self.agent_bytes },
            "profile": self.profile_id,
            "surface": self.surface,
            "verdict": self.verdict().as_str(),
            "token": issued.as_ref().map(|found| found.token.clone()),
            "clearance": issued.as_ref().and_then(|found| found.clearance.clone()),
            "answer": issued.as_ref().map(|found| json!({ "status": found.status, "headers": found.headers })),
            "payload_bytes": issued.as_ref().map(|found| found.payload_bytes).unwrap_or_default(),
            "threw": threw,
            "waited_ms": waited,
            "ms": started.elapsed().as_millis() as u64,
            "sent": self.sent(),
            "misses": self.misses(),
            "cookies": self.cookie_pairs(),
        }))
    }

    fn settle_until_issued(&mut self) -> ClientResult<u64> {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(self.settings.wait_ms as u64);
        let step = Duration::from_millis(self.settings.step_ms.max(1.0) as u64);

        while Instant::now() < deadline && self.issued().is_none() {
            self.pump()?;
            std::thread::sleep(step);
        }

        for _ in 0..8 {
            if self.pump()? == 0 {
                break;
            }
        }

        Ok(started.elapsed().as_millis() as u64)
    }

    pub fn pump(&mut self) -> ClientResult<usize> {
        let ran = self
            .browser_mut()?
            .step()
            .map_err(|error| ClientError::internal(format!("the sandbox stalled: {error}")))?;

        self.sync_cookies();
        Ok(ran)
    }

    fn sync_cookies(&mut self) {
        let url = self.page_url.clone();
        let Ok(header) = self.browser_mut().and_then(|browser| {
            browser
                .cookies()
                .map_err(|error| ClientError::internal(error.to_string()))
        }) else {
            return;
        };

        for pair in header.split(';') {
            let trimmed = pair.trim();
            if trimmed.is_empty() {
                continue;
            }

            let _ = self.jar.add(&url, trimmed);
        }
    }

    pub fn load_loader(&mut self, entries: &Value) -> ClientResult<Value> {
        let Some(tenant) = self.surface.tenant.clone() else {
            return Err(ClientError::unsupported("this page names no Kasada tenant"));
        };

        let url = tenant.endpoint("p.js");
        let source = self.fetch_script(&url)?;

        self.run(
            "try { delete window.KPSDK; } catch (error) {}",
            "kasada:reset",
        )?;
        let threw = self.run(&source, &url)?;

        self.run(
            &format!(
                "window.KPSDK.configure({});",
                serde_json::to_string(entries).unwrap_or_default()
            ),
            "kasada:configure",
        )?;

        self.loader = true;

        Ok(json!({ "url": url, "bytes": source.len(), "threw": threw }))
    }

    pub fn loaded(&self) -> bool {
        self.loader
    }

    pub fn stamped(
        &mut self,
        url: &str,
        method: &str,
        headers: &Value,
        body: Option<&str>,
    ) -> ClientResult<Value> {
        let spec = json!({
            "url": url,
            "method": method.to_uppercase(),
            "headers": headers,
            "body": body,
        });

        let source = format!(
            r#"globalThis.__wreAnswer = null;
(function () {{
  var spec = {spec};
  var options = {{ method: spec.method, headers: spec.headers, credentials: "include" }};
  if (spec.body !== null && spec.body !== undefined) options.body = spec.body;

  fetch(spec.url, options).then(function (response) {{
    return response.text().then(function (text) {{
      var headers = {{}};
      response.headers.forEach(function (value, name) {{ headers[name] = value; }});
      globalThis.__wreAnswer = {{ status: response.status, headers: headers, bytes: text.length, body: text }};
    }});
  }}, function (error) {{
    globalThis.__wreAnswer = {{ status: 0, headers: {{}}, bytes: 0, body: String(error && error.message) }};
  }});
}})();"#
        );

        self.run(&source, "kasada:stamped")?;

        for _ in 0..200 {
            self.pump()?;

            let answer = self
                .browser_mut()?
                .read("globalThis.__wreAnswer")
                .map_err(|error| {
                    ClientError::internal(format!("the answer did not come back: {error}"))
                })?;

            if !answer.is_null() {
                return Ok(answer);
            }

            std::thread::sleep(Duration::from_millis(50));
        }

        Err(ClientError::timeout("the stamped request never answered"))
    }

    pub fn issued(&self) -> Option<Issued> {
        let transport = self.transport.as_ref()?;
        let slot = transport
            .issued
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        slot.clone()
    }

    pub fn payload(&self) -> Option<Vec<u8>> {
        let transport = self.transport.as_ref()?;
        let slot = transport
            .payload
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        slot.clone()
    }

    pub fn vector(&mut self) -> ClientResult<Value> {
        let log = self.browser_mut()?.log().map_err(|error| {
            ClientError::internal(format!("the log did not come back: {error}"))
        })?;

        let Some(entries) = log.as_array() else {
            return Ok(Value::Null);
        };

        let last = entries
            .iter()
            .rfind(|entry| entry.get("kind").and_then(Value::as_str) == Some("vector"));

        let Some(found) = last
            .and_then(|entry| entry.get("detail"))
            .and_then(Value::as_str)
        else {
            return Ok(Value::Null);
        };

        Ok(serde_json::from_str(found).unwrap_or(Value::Null))
    }

    pub fn reports(&self) -> Vec<Value> {
        match &self.transport {
            Some(transport) => {
                let list = transport
                    .reports
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                list.clone()
            }
            None => Vec::new(),
        }
    }

    pub fn verdict(&self) -> Verdict {
        match self.issued() {
            Some(found) => token::verdict(Some(&found.token), Some(found.accepted)),
            None => Verdict::None,
        }
    }

    pub fn sent(&self) -> Vec<Sent> {
        match &self.transport {
            Some(transport) => {
                let list = transport
                    .sent
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                list.clone()
            }
            None => Vec::new(),
        }
    }

    pub fn misses(&mut self) -> Vec<String> {
        match self.browser.as_mut() {
            Some(browser) => browser.misses(),
            None => Vec::new(),
        }
    }

    pub fn guards(&mut self) -> Vec<String> {
        match self.browser.as_mut() {
            Some(browser) => browser.guards(),
            None => Vec::new(),
        }
    }

    pub fn close(&mut self) {
        self.browser = None;
        self.transport = None;
        self.loader = false;
    }
}

fn response_url(response: &FetchResponse) -> &str {
    &response.url
}
