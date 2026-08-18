use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex};

use regex::Regex;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use wre_behavior::stream::{Point, Shape, Stream};
use wre_client::context::{Claim, FetchRequest, FetchResponse, Http, Jar};
use wre_client::error::{ClientError, ClientResult};
use wre_live::realm::RealmOptions;
use wre_sandbox::machine;
use wre_sandbox::browser::{Answer, Browser, CookieStore, Hooks, Request, Transport, now_ms, open};
use wre_sandbox::page::Page;
use wre_sandbox::profile::Profile;

use crate::cookies::{self, Summary};
use crate::discover::{Surface, discover};
use crate::sensor;

const NAVIGATE: [(&str, &str); 7] = [
    (
        "accept",
        "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
    ),
    ("priority", "u=0, i"),
    ("sec-fetch-dest", "document"),
    ("sec-fetch-mode", "navigate"),
    ("sec-fetch-site", "none"),
    ("sec-fetch-user", "?1"),
    ("upgrade-insecure-requests", "1"),
];

const SCRIPT: [(&str, &str); 4] = [
    ("accept", "*/*"),
    ("sec-fetch-dest", "script"),
    ("sec-fetch-mode", "no-cors"),
    ("sec-fetch-site", "same-origin"),
];

const STYLE: [(&str, &str); 4] = [
    ("accept", "text/css,*/*;q=0.1"),
    ("sec-fetch-dest", "style"),
    ("sec-fetch-mode", "no-cors"),
    ("sec-fetch-site", "same-origin"),
];

const FONT: [(&str, &str); 4] = [
    ("accept", "*/*"),
    ("sec-fetch-dest", "font"),
    ("sec-fetch-mode", "cors"),
    ("sec-fetch-site", "same-origin"),
];

const IMAGE: [(&str, &str); 4] = [
    ("accept", "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8"),
    ("sec-fetch-dest", "image"),
    ("sec-fetch-mode", "no-cors"),
    ("sec-fetch-site", "same-origin"),
];

const NAVIGATE_ORDER: [&str; 14] = [
    "sec-ch-ua",
    "sec-ch-ua-mobile",
    "sec-ch-ua-platform",
    "upgrade-insecure-requests",
    "user-agent",
    "accept",
    "sec-fetch-site",
    "sec-fetch-mode",
    "sec-fetch-user",
    "sec-fetch-dest",
    "accept-encoding",
    "accept-language",
    "cookie",
    "priority",
];

const SCRIPT_ORDER: [&str; 13] = [
    "sec-ch-ua-platform",
    "user-agent",
    "sec-ch-ua",
    "sec-ch-ua-mobile",
    "accept",
    "sec-fetch-site",
    "sec-fetch-mode",
    "sec-fetch-dest",
    "referer",
    "accept-encoding",
    "accept-language",
    "cookie",
    "priority",
];

pub const FORM: [(&str, &str); 9] = [
    (
        "accept",
        "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
    ),
    ("cache-control", "max-age=0"),
    ("content-type", "application/x-www-form-urlencoded"),
    ("priority", "u=0, i"),
    ("sec-fetch-dest", "document"),
    ("sec-fetch-mode", "navigate"),
    ("sec-fetch-site", "same-origin"),
    ("sec-fetch-user", "?1"),
    ("upgrade-insecure-requests", "1"),
];

pub const FORM_ORDER: [&str; 19] = [
    "content-length",
    "cache-control",
    "sec-ch-ua",
    "sec-ch-ua-mobile",
    "sec-ch-ua-platform",
    "upgrade-insecure-requests",
    "content-type",
    "user-agent",
    "origin",
    "accept",
    "sec-fetch-site",
    "sec-fetch-mode",
    "sec-fetch-user",
    "sec-fetch-dest",
    "referer",
    "accept-encoding",
    "accept-language",
    "cookie",
    "priority",
];

pub const XHR_ORDER: [&str; 16] = [
    "content-length",
    "sec-ch-ua-platform",
    "user-agent",
    "sec-ch-ua",
    "content-type",
    "sec-ch-ua-mobile",
    "accept",
    "origin",
    "sec-fetch-site",
    "sec-fetch-mode",
    "sec-fetch-dest",
    "referer",
    "accept-encoding",
    "accept-language",
    "cookie",
    "priority",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub wait_ms: f64,
    pub init_cost_ms: f64,
    pub friction_ms: f64,
    pub script_rate: f64,
    pub behaviour: bool,
    pub pixel: bool,
    pub live_xhr: bool,
    pub wire_pace: bool,
    pub keep_payloads: bool,
    pub timeout_ms: u64,
    pub seed: u64,
    pub prelude: Option<String>,
    pub load_posts_ms: f64,
    pub warp: f64,
    pub typed: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            wait_ms: 6_000.0,
            init_cost_ms: 25.0,
            friction_ms: 0.12,
            script_rate: 0.07,
            behaviour: true,
            pixel: true,
            live_xhr: true,
            wire_pace: false,
            keep_payloads: false,
            timeout_ms: 90_000,
            seed: 0,
            prelude: None,
            load_posts_ms: 4_000.0,
            warp: 16.0,
            typed: String::new(),
        }
    }
}

pub fn accept_language(profile: &Profile) -> String {
    let listed: Vec<String> = profile
        .property("Navigator", "languages")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    if listed.is_empty() {
        return "en-US,en;q=0.9".to_string();
    }

    listed
        .iter()
        .enumerate()
        .map(|(at, language)| match at {
            0 => language.clone(),
            _ => format!("{language};q={:.1}", 1.0 - (at as f64) * 0.1),
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub fn claim_of(profile: &Profile, user_agent: &str) -> Option<Claim> {
    let data = profile.user_agent_data.as_ref()?;

    if data.brands.is_empty() || user_agent.is_empty() {
        return None;
    }

    let brands = data
        .brands
        .iter()
        .map(|entry| format!("\"{}\";v=\"{}\"", entry.brand, entry.version))
        .collect::<Vec<_>>()
        .join(", ");

    Some(Claim {
        user_agent: user_agent.to_string(),
        brands,
        mobile: data.mobile,
        platform: data.platform.clone(),
    })
}

fn pointer_shape() -> Shape {
    Shape {
        step_px: 6.5,
        sample_ms: 18.0,
        jitter_px: 1.0,
        overshoot: 0.05,
        dwell_ms: 120.0,
        flight_ms: 260.0,
        pause_ms: 520.0,
    }
}

fn base_of(url: &str) -> Result<Url, ()> {
    Url::parse(url).map_err(|_| ())
}

fn is_font(url: &str) -> bool {
    [".woff2", ".woff", ".ttf", ".otf", ".eot", ".svg"].iter().any(|suffix| suffix_of(url) == *suffix)
}

fn suffix_of(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
    match path.rfind('.') {
        Some(dot) => path[dot..].to_string(),
        None => String::new(),
    }
}

fn referenced(css: &str) -> Vec<String> {
    static URL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?i)url\(\s*['"]?([^'")]+?)['"]?\s*\)"#).expect("css url pattern")
    });

    let mut found = Vec::new();

    for reference in URL.captures_iter(css) {
        let value = reference[1].trim().to_string();
        if value.starts_with("data:") || value.is_empty() || found.contains(&value) {
            continue;
        }
        found.push(value);
    }

    found
}

fn families_in_use(css: &str, markup: &str) -> Vec<String> {
    static FACE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?is)@font-face\s*\{(.*?)\}"#).expect("font face pattern")
    });

    static FAMILY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?is)font-family\s*:\s*['"]?([^;'"}]+)"#).expect("family pattern")
    });

    static RULE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?is)([^{}]+)\{([^{}]*)\}"#).expect("rule pattern")
    });

    static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?i)[.#]?[a-z0-9_-]+"#).expect("token pattern")
    });

    let declared: Vec<String> = FACE
        .captures_iter(css)
        .filter_map(|face| FAMILY.captures(&face[1]).map(|found| found[1].trim().to_lowercase()))
        .collect();

    if declared.is_empty() {
        return Vec::new();
    }

    let without_faces = FACE.replace_all(css, "");
    let mut used = Vec::new();

    for rule in RULE.captures_iter(&without_faces) {
        let Some(asked) = FAMILY.captures(&rule[2]) else { continue };
        let asked = asked[1].trim().to_lowercase();

        let Some(family) = declared.iter().find(|name| asked.contains(name.as_str())) else {
            continue;
        };

        if used.contains(family) {
            continue;
        }

        for token in TOKEN.find_iter(&rule[1]) {
            let token = token.as_str();
            let matched = if let Some(class) = token.strip_prefix('.') {
                markup.contains(&format!("{class} ")) || markup.contains(&format!("{class}\""))
            } else if let Some(id) = token.strip_prefix('#') {
                markup.contains(&format!("id=\"{id}\""))
            } else {
                markup.contains(&format!("<{token}"))
            };

            if matched {
                used.push(family.clone());
                break;
            }
        }
    }

    used
}

#[derive(Debug, Clone)]
pub struct Want {
    pub url: String,
    pub kind: Asset,
    pub priority: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asset {
    Style,
    Script,
    Image,
    Font,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sent {
    pub url: String,
    pub status: u16,
    pub bytes: usize,
    pub live: bool,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abck: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<String>,
    #[serde(default)]
    pub at: f64,
}

struct Wire {
    http: Arc<Http>,
    jar: Jar,
    page_url: String,
    origin: String,
    referer: String,
    user_agent: String,
    hints: Vec<(String, String)>,
    languages: String,
    enabled: bool,
    keep_payloads: bool,
    pace: bool,
    clock: Mutex<Option<(f64, f64)>>,
    sent: Mutex<Vec<Sent>>,
}

struct Live {
    wire: Arc<Wire>,
    flight: Mutex<BTreeMap<u64, std::sync::mpsc::Receiver<Answer>>>,
    next: std::sync::atomic::AtomicU64,
}

impl Live {
    fn pending(&self) -> usize {
        self.flight
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len()
    }

    fn new(wire: Wire) -> Self {
        Self {
            wire: Arc::new(wire),
            flight: Mutex::new(BTreeMap::new()),
            next: std::sync::atomic::AtomicU64::new(1),
        }
    }
}

impl Wire {
    fn abck(&self) -> Option<String> {
        let pairs: Vec<(String, String)> = self
            .jar
            .matching(&self.page_url)
            .into_iter()
            .map(|cookie| (cookie.name, cookie.value))
            .collect();

        cookies::summarise(&pairs).abck.map(|abck| abck.status)
    }

    fn same_origin(&self, url: &str) -> bool {
        match (Url::parse(url), Url::parse(&self.origin)) {
            (Ok(target), Ok(origin)) => target.host_str() == origin.host_str(),
            _ => false,
        }
    }
}

impl Wire {
    fn kept(&self, request: &Request) -> Option<String> {
        if !self.keep_payloads {
            return None;
        }

        request
            .body
            .as_deref()
            .and_then(|body| sensor::extract(body).or_else(|| sensor::payload_of(body)))
    }
}

impl Transport for Live {
    fn send(&self, request: &Request) -> Answer {
        self.wire.send(request)
    }

    fn start(&self, request: &Request) -> Option<u64> {
        if !self.wire.enabled {
            return None;
        }

        let ticket = self
            .next
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let (answered, waiting) = std::sync::mpsc::channel();
        let wire = Arc::clone(&self.wire);
        let request = request.clone();

        std::thread::spawn(move || {
            let _ = answered.send(wire.send(&request));
        });

        self.flight
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(ticket, waiting);

        Some(ticket)
    }

    fn take(&self, ticket: u64) -> Option<Answer> {
        let mut flight = self.flight.lock().unwrap_or_else(|error| error.into_inner());
        let answer = flight.get(&ticket)?.try_recv().ok()?;
        flight.remove(&ticket);
        Some(answer)
    }
}

impl Wire {
    fn send(&self, request: &Request) -> Answer {
        let bytes = request.body.as_ref().map(String::len).unwrap_or_default();

        if !self.enabled {
            let mut sent = self.sent.lock().unwrap_or_else(|error| error.into_inner());
            sent.push(Sent {
                url: request.url.clone(),
                status: 0,
                bytes,
                live: false,
                source: request.source.clone(),
                payload: self.kept(request),
                abck: None,
                at: now_ms(),
                headers: Vec::new(),
            });
            return Answer::default();
        }

        let mut outgoing = FetchRequest {
            url: request.url.clone(),
            method: request.method.to_uppercase(),
            headers: Vec::new(),
            body: request.body.as_ref().map(|body| body.as_bytes().to_vec()),
            fingerprint: None,
            order: XHR_ORDER.iter().map(|name| name.to_string()).collect(),
        };

        let cross = !self.same_origin(&request.url);

        let mut headers: BTreeMap<String, String> = BTreeMap::from([
            ("accept".to_string(), "*/*".to_string()),
            ("accept-encoding".to_string(), "gzip, deflate, br, zstd".to_string()),
            ("accept-language".to_string(), self.languages.clone()),
            ("priority".to_string(), "u=1, i".to_string()),
            ("sec-fetch-dest".to_string(), "empty".to_string()),
            ("sec-fetch-mode".to_string(), "cors".to_string()),
            (
                "sec-fetch-site".to_string(),
                if cross { "cross-site".to_string() } else { "same-origin".to_string() },
            ),
            ("user-agent".to_string(), self.user_agent.clone()),
        ]);

        if cross || outgoing.method != "GET" {
            headers.insert("origin".to_string(), self.origin.clone());
        }

        headers.insert(
            "referer".to_string(),
            if cross { format!("{}/", self.origin) } else { self.referer.clone() },
        );

        headers.extend(self.hints.iter().cloned());

        for (name, value) in &request.headers {
            headers.insert(name.to_lowercase(), value.clone());
        }

        let names: Vec<String> = XHR_ORDER
            .iter()
            .filter(|name| **name == "content-length" || **name == "cookie" || headers.contains_key(**name))
            .map(|name| name.to_string())
            .collect();

        outgoing.headers = headers.into_iter().collect();

        let mut paced = 0.0;

        if self.pace {
            let mut clock = self.clock.lock().unwrap_or_else(|error| error.into_inner());

            if let Some((last_at, last_wall)) = *clock {
                let ahead = last_wall + (request.at - last_at) - now_ms();
                if ahead > 0.0 {
                    paced = ahead.min(6_000.0);
                    std::thread::sleep(std::time::Duration::from_millis(paced as u64));
                }
            }

            *clock = Some((request.at, now_ms()));
        }

        let answer = match self.http.fetch(outgoing) {
            Ok(response) => Answer {
                status: response.status,
                body: response.text(),
                headers: response
                    .headers
                    .iter()
                    .filter(|(name, _)| !name.eq_ignore_ascii_case("set-cookie"))
                    .cloned()
                    .collect(),
                paced,
            },
            Err(_) => Answer { status: 0, body: String::new(), headers: Vec::new(), paced },
        };

        let mut sent = self.sent.lock().unwrap_or_else(|error| error.into_inner());
        sent.push(Sent {
            url: request.url.clone(),
            status: answer.status,
            bytes,
            live: true,
            source: request.source.clone(),
            payload: self.kept(request),
            abck: self.abck(),
            at: now_ms(),
            headers: if self.keep_payloads { names } else { Vec::new() },
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
    profile: Profile,
    profile_id: String,
    settings: Settings,
    user_agent: String,
    hints: Vec<(String, String)>,
    languages: String,
    page_url: String,
    html: String,
    surface: Surface,
    sensor_url: String,
    sensor_bytes: usize,
    sensor_source: String,
    loader_source: String,
    page_scripts: Vec<PageScript>,
    inline_scripts: (Vec<String>, Vec<String>),
    browser: Option<Browser>,
    transport: Option<Arc<Live>>,
    posts: Vec<Sent>,
    timings: Vec<Value>,
    trail: Mutex<Vec<Value>>,
}

struct PageScript {
    url: String,
    source: String,
    deferred: bool,
    before_sensor: bool,
}

impl Session {
    pub fn new(
        http: Arc<Http>,
        jar: Jar,
        profile: Profile,
        profile_id: String,
        settings: Settings,
        user_agent: String,
    ) -> Self {
        let hints = claim_of(&profile, &user_agent)
            .map(|claim| claim.hints())
            .unwrap_or_default();
        let languages = accept_language(&profile);

        Self {
            http,
            jar,
            profile,
            profile_id,
            settings,
            user_agent,
            hints,
            languages,
            page_url: String::new(),
            html: String::new(),
            surface: Surface::default(),
            sensor_url: String::new(),
            sensor_bytes: 0,
            sensor_source: String::new(),
            loader_source: String::new(),
            page_scripts: Vec::new(),
            inline_scripts: (Vec::new(), Vec::new()),
            browser: None,
            transport: None,
            posts: Vec::new(),
            timings: Vec::new(),
            trail: Mutex::new(Vec::new()),
        }
    }

    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    pub fn page_url(&self) -> &str {
        &self.page_url
    }

    pub fn html(&self) -> &str {
        &self.html
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

    pub fn cookie_pairs(&self) -> Vec<(String, String)> {
        let url = if self.page_url.is_empty() { "https://localhost/" } else { &self.page_url };

        self.jar
            .matching(url)
            .into_iter()
            .map(|cookie| (cookie.name, cookie.value))
            .collect()
    }

    pub fn cookies(&self) -> Summary {
        cookies::summarise(&self.cookie_pairs())
    }

    pub fn fetch(&self, request: FetchRequest) -> ClientResult<FetchResponse> {
        let url = request.url.clone();
        let method = request.method.clone();
        let answer = self.http.fetch(request);

        let mut trail = self.trail.lock().unwrap_or_else(|error| error.into_inner());
        trail.push(json!({
            "at": now_ms(),
            "method": method,
            "url": url,
            "status": answer.as_ref().map(|found| found.status).unwrap_or(0),
        }));

        answer
    }

    pub fn trail(&self) -> Vec<Value> {
        self.trail.lock().unwrap_or_else(|error| error.into_inner()).clone()
    }

    pub fn navigate(&mut self, url: &str) -> ClientResult<FetchResponse> {
        let mut request = FetchRequest::get(url)
            .header("user-agent", self.user_agent.clone())
            .header("accept-language", self.languages.clone())
            .ordered(&NAVIGATE_ORDER);
        for (name, value) in NAVIGATE {
            request = request.header(name, value);
        }

        let response = self.fetch(request)?;

        self.page_url = response.url.clone();
        self.html = response.text();
        self.surface = discover(&self.html, &self.page_url);

        Ok(response)
    }

    pub fn fetch_script(&mut self, url: &str) -> ClientResult<String> {
        let mut request = FetchRequest::get(url)
            .header("user-agent", self.user_agent.clone())
            .header("accept-language", self.languages.clone())
            .header("referer", self.page_url.clone())
            .ordered(&SCRIPT_ORDER);

        for (name, value) in SCRIPT {
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

    pub fn mount(&mut self) -> ClientResult<()> {
        let origin = Url::parse(&self.page_url)
            .ok()
            .map(|parsed| {
                format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or_default())
            })
            .unwrap_or_default();

        let transport = Arc::new(Live::new(Wire {
            http: Arc::clone(&self.http),
            jar: self.jar.clone(),
            page_url: self.page_url.clone(),
            origin,
            referer: self.page_url.clone(),
            user_agent: self.user_agent.clone(),
            hints: self.hints.clone(),
            languages: self.languages.clone(),
            enabled: self.settings.live_xhr,
            keep_payloads: self.settings.keep_payloads,
            pace: self.settings.wire_pace,
            clock: Mutex::new(None),
            sent: Mutex::new(Vec::new()),
        }));

        let hooks = Hooks {
            transport: Arc::clone(&transport) as Arc<dyn Transport>,
            cookies: Arc::new(Cookies { jar: self.jar.clone(), url: self.page_url.clone() }),
        };

        let mut page = Page::read(&self.page_url, &self.html)
            .with_epoch(now_ms())
            .with_friction(self.settings.friction_ms);

        if let Some(geometry) = self.geometry() {
            page = page.with_geometry(geometry);
        }

        if !self.sensor_url.is_empty() {
            page = page.running(&self.sensor_url);
        }

        let options = RealmOptions {
            timeout: std::time::Duration::from_millis(self.settings.timeout_ms.max(1000)),
            timers: false,
            codecs: true,
            clock_ms: None,
            random_seed: std::env::var("WRE_AKAMAI_RANDOM_SEED").ok().and_then(|value| value.parse().ok()),
            heap_limit_mb: None,
        };

        self.inline_scripts = page.inline_scripts();

        let mut browser = open(&self.profile, &page, hooks, options)
            .map_err(|error| ClientError::internal(format!("the sandbox did not open: {error}")))?;

        browser
            .charge_on("bmak", "startTs", self.settings.init_cost_ms)
            .map_err(|error| ClientError::internal(format!("the clock charge failed: {error}")))?;

        self.browser = Some(browser);
        self.transport = Some(transport);
        Ok(())
    }

    fn geometry(&self) -> Option<Value> {
        let host = Url::parse(&self.page_url).ok()?.host_str()?.to_string();
        let workspace = wre_core::paths::Workspace::discover().ok()?;
        let path = workspace.artifacts().join("geometry").join(format!("{host}.json"));
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn assets(&self) -> Vec<Want> {
        static TAG: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?is)<(link|script|img)\s([^>]*?)/?>"#).expect("tag pattern")
        });

        static ATTR: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?is)([a-z-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("attribute pattern")
        });

        static NOSCRIPT: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?is)<noscript[^>]*>.*?</noscript\s*>"#).expect("noscript pattern")
        });

        let Ok(base) = Url::parse(&self.page_url) else {
            return Vec::new();
        };

        let scripted = NOSCRIPT.replace_all(&self.html, "");
        let mut found: Vec<Want> = Vec::new();
        let mut blocking = 0;

        for tag in TAG.captures_iter(&scripted) {
            let name = tag[1].to_ascii_lowercase();
            let mut attributes: BTreeMap<String, String> = BTreeMap::new();

            for attribute in ATTR.captures_iter(&tag[2]) {
                let value = attribute
                    .get(2)
                    .or_else(|| attribute.get(3))
                    .map(|part| part.as_str().to_string())
                    .unwrap_or_default();
                attributes.insert(attribute[1].to_ascii_lowercase(), value);
            }

            let rel = attributes.get("rel").cloned().unwrap_or_default().to_ascii_lowercase();

            let deferred = attributes.contains_key("defer") || attributes.contains_key("async");

            let (reference, kind) = match name.as_str() {
                "script" => (attributes.get("src"), Asset::Script),
                "img" => (attributes.get("src"), Asset::Image),
                "link" if rel.contains("stylesheet") => (attributes.get("href"), Asset::Style),
                "link" if rel.contains("icon") => (attributes.get("href"), Asset::Image),
                _ => (None, Asset::Image),
            };

            let Some(reference) = reference else { continue };
            let Ok(resolved) = base.join(reference) else { continue };
            let resolved = resolved.to_string();

            if resolved == self.sensor_url || found.iter().any(|want| want.url == resolved) {
                continue;
            }

            let priority = match kind {
                Asset::Style => Some("u=0".to_string()),
                Asset::Image => Some("u=2, i".to_string()),
                Asset::Font => Some("u=0".to_string()),
                Asset::Script if deferred => None,
                Asset::Script => {
                    blocking += 1;
                    Some(if blocking == 1 { "u=1".to_string() } else { "u=2".to_string() })
                }
            };

            found.push(Want { url: resolved, kind, priority });
        }

        static INLINE_URL: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?is)style\s*=\s*(?:"[^"]*?url\(\s*['"]?([^'")]+)|'[^']*?url\(\s*["']?([^'")]+))"#)
                .expect("inline url pattern")
        });

        for style in INLINE_URL.captures_iter(&scripted) {
            let Some(reference) = style.get(1).or_else(|| style.get(2)) else { continue };
            let Ok(resolved) = base.join(reference.as_str().trim()) else { continue };
            let resolved = resolved.to_string();

            if resolved == self.sensor_url || found.iter().any(|want| want.url == resolved) {
                continue;
            }

            found.push(Want {
                url: resolved,
                kind: Asset::Image,
                priority: Some("u=2, i".to_string()),
            });
        }

        found
    }

    fn fetch_batch(&self, wanted: &[Want]) -> Vec<ClientResult<FetchResponse>> {
        let requests: Vec<FetchRequest> = wanted
            .iter()
            .map(|want| {
                let (url, kind) = (&want.url, &want.kind);
                let headers: &[(&str, &str)] = match kind {
                    Asset::Style => &STYLE,
                    Asset::Image => &IMAGE,
                    Asset::Script => &SCRIPT,
                    Asset::Font => &FONT,
                };

                let mut request = FetchRequest::get(url)
                    .header("user-agent", self.user_agent.clone())
                    .header("accept-language", self.languages.clone())
                    .header("referer", self.page_url.clone())
                    .ordered(&SCRIPT_ORDER);

                if let Some(priority) = &want.priority {
                    request = request.header("priority", priority.clone());
                }

                if *kind == Asset::Font {
                    if let Some(origin) = base_of(&self.page_url).ok().map(|base| base.origin().ascii_serialization()) {
                        request = request.header("origin", origin);
                    }
                }

                for (name, value) in headers {
                    request = request.header(*name, *value);
                }

                request
            })
            .collect();

        let answers = self.http.fetch_together(requests);

        let mut trail = self.trail.lock().unwrap_or_else(|error| error.into_inner());
        for (want, answer) in wanted.iter().zip(answers.iter()) {
            trail.push(json!({
                "at": now_ms(),
                "method": "GET",
                "url": want.url,
                "status": answer.as_ref().map(|found| found.status).unwrap_or(0),
            }));
        }
        drop(trail);

        answers
    }

    fn fetch_page(&mut self, sensor_url: &str) -> ClientResult<(String, usize)> {
        let mut wanted = self.assets();

        if !wanted.iter().any(|want| want.url == sensor_url) {
            wanted.insert(
                0,
                Want {
                    url: sensor_url.to_string(),
                    kind: Asset::Script,
                    priority: Some("u=2".to_string()),
                },
            );
        }

        let at = wanted
            .iter()
            .position(|want| want.url == sensor_url)
            .expect("the sensor script is in the batch");

        let answers = self.fetch_batch(&wanted);
        let mut fetched = answers.iter().filter(|answer| answer.is_ok()).count();

        let loader = self.surface.pixel_client.as_ref().map(|found| found.url.clone());

        if let Some(loader) = loader.as_ref() {
            if let Some(position) = wanted.iter().position(|want| want.url == *loader) {
                if let Ok(response) = &answers[position] {
                    self.loader_source = response.text();
                }
            }
        }

        self.page_scripts = wanted
            .iter()
            .zip(answers.iter())
            .enumerate()
            .filter(|(_, (want, _))| want.kind == Asset::Script)
            .filter(|(_, (want, _))| want.url != sensor_url && Some(&want.url) != loader.as_ref())
            .filter_map(|(index, (want, answer))| {
                let response = answer.as_ref().ok()?;
                if response.status != 200 {
                    return None;
                }
                Some(PageScript {
                    url: want.url.clone(),
                    source: response.text(),
                    deferred: want.priority.is_none(),
                    before_sensor: index < at,
                })
            })
            .collect();

        let mut second: Vec<Want> = Vec::new();

        for (want, answer) in wanted.iter().zip(answers.iter()) {
            if want.kind != Asset::Style {
                continue;
            }

            let Ok(response) = answer else { continue };
            let Ok(sheet) = Url::parse(&want.url) else { continue };

            let sheet_text = response.text();
            let wanted_families = families_in_use(&sheet_text, &self.html);

            for reference in referenced(&sheet_text) {
                let Ok(resolved) = sheet.join(&reference) else { continue };
                let resolved = resolved.to_string();

                if second.iter().any(|seen| seen.url == resolved) {
                    continue;
                }

                if is_font(&resolved) {
                    let plain = |value: &str| {
                        value
                            .to_lowercase()
                            .chars()
                            .filter(|character| character.is_ascii_alphanumeric())
                            .collect::<String>()
                    };
                    let target = plain(&resolved);
                    let asked = wanted_families.iter().any(|family| target.contains(&plain(family)));

                    if asked && suffix_of(&resolved) == ".woff2" {
                        second.push(Want {
                            url: resolved.clone(),
                            kind: Asset::Font,
                            priority: Some("u=0".to_string()),
                        });
                    }
                    continue;
                }

                second.push(Want {
                    url: resolved.clone(),
                    kind: Asset::Image,
                    priority: Some("i".to_string()),
                });
            }
        }

        if !self.html.contains("rel=\"icon\"") && !self.html.contains("rel=\"shortcut icon\"") {
            if let Ok(icon) = base_of(&self.page_url).and_then(|base| base.join("/favicon.ico").map_err(|_| ())) {
                second.push(Want {
                    url: icon.to_string(),
                    kind: Asset::Image,
                    priority: Some("u=1, i".to_string()),
                });
            }
        }

        if !second.is_empty() {
            fetched += self.fetch_batch(&second).iter().filter(|answer| answer.is_ok()).count();
        }

        let source = match &answers[at] {
            Ok(response) if response.status == 200 => response.text(),
            Ok(response) => {
                return Err(ClientError::resource(format!(
                    "the script at {sensor_url} answered {}",
                    response.status
                )));
            }
            Err(error) => return Err(ClientError::resource(format!("{sensor_url}: {error}"))),
        };

        Ok((source, fetched))
    }

    pub fn open(&mut self, url: &str) -> ClientResult<Value> {
        let began = std::time::Instant::now();
        let page = self.navigate(url)?;
        self.spent("akamai:navigate", began);

        if page.status >= 400 {
            return Err(ClientError::resource(format!(
                "{url} answered {} before any script ran",
                page.status
            )));
        }

        let Some(sensor) = self.surface.sensor.clone() else {
            return Err(ClientError::unsupported(format!(
                "{url} names no Akamai sensor script"
            )));
        };

        let began = std::time::Instant::now();
        let (source, assets) = self.fetch_page(&sensor.url)?;
        self.spent("akamai:assets", began);

        if source.len() < 1000 {
            return Err(ClientError::resource(format!(
                "the sensor script is only {} bytes, that is not a build",
                source.len()
            )));
        }

        self.sensor_url = sensor.url.clone();
        self.sensor_bytes = source.len();
        self.sensor_source = source.clone();

        let began = std::time::Instant::now();
        self.mount()?;
        self.spent("akamai:mount", began);

        if let Some(prelude) = self.settings.prelude.clone() {
            self.run(&prelude, "akamai:prelude")?;
        }

        let scripts = std::mem::take(&mut self.page_scripts);
        let mut ran = Vec::new();

        let execute = |session: &mut Self, script: &PageScript, ran: &mut Vec<Value>| {
            let threw = session.run(&script.source, &format!("akamai:page:{}", script.url))?;
            ran.push(json!({ "url": script.url, "deferred": script.deferred, "threw": threw }));
            Ok::<(), ClientError>(())
        };

        for script in scripts.iter().filter(|script| !script.deferred && script.before_sensor) {
            execute(self, script, &mut ran)?;
        }

        let (before, after) = std::mem::take(&mut self.inline_scripts);

        for (index, source) in before.iter().enumerate() {
            let threw = self.run(source, &format!("akamai:inline:{index}"))?;
            ran.push(json!({ "url": format!("[inline {index}]"), "deferred": false, "threw": threw }));
        }

        let threw = self.run(&source, "akamai:sensor")?;

        for script in scripts.iter().filter(|script| !script.deferred && !script.before_sensor) {
            execute(self, script, &mut ran)?;
        }

        for (index, source) in after.iter().enumerate() {
            let at = before.len() + index;
            let threw = self.run(source, &format!("akamai:inline:{at}"))?;
            ran.push(json!({ "url": format!("[inline {at}]"), "deferred": false, "threw": threw }));
        }

        if let Some(browser) = self.browser.as_mut() {
            browser
                .finish_parse()
                .map_err(|error| ClientError::internal(format!("the sandbox stalled: {error}")))?;
        }

        let began = std::time::Instant::now();
        let pixel = if self.settings.pixel { self.run_pixel()? } else { Value::Null };
        self.spent("akamai:pixel-client", began);

        for script in scripts.iter().filter(|script| script.deferred) {
            execute(self, script, &mut ran)?;
        }

        let began = std::time::Instant::now();
        self.settle()?;
        self.spent("akamai:settle", began);

        let browser = self.browser.as_mut().expect("mounted");
        let misses = browser.misses();
        let errors = browser.errors();
        let requests = browser.requests().len();

        let sent = self.taken();
        self.posts.extend(sent.iter().cloned());

        Ok(json!({
            "page": { "url": self.page_url, "status": page.status, "bytes": self.html.len(), "assets": assets, "scripts": ran },
            "trail": self.trail(),
            "sensor": { "url": self.sensor_url, "bytes": self.sensor_bytes },
            "profile": self.profile_id,
            "machine": machine::describe(&self.profile),
            "surface": self.surface,
            "pixel": pixel,
            "threw": threw,
            "requests": requests,
            "sent": sent,
            "timings": self.timings.clone(),
            "misses": misses,
            "errors": errors,
            "cookies": self.cookies(),
        }))
    }

    fn run(&mut self, source: &str, name: &str) -> ClientResult<Value> {
        let started = std::time::Instant::now();
        let running_src = match name {
            "akamai:sensor" => self.sensor_url.clone(),
            "akamai:pixel" => self.surface.pixel_client.as_ref().map(|entry| entry.url.clone()).unwrap_or_default(),
            other => other.strip_prefix("akamai:page:").unwrap_or_default().to_string(),
        };

        let browser = self
            .browser
            .as_mut()
            .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

        browser.running_script(&running_src).ok();
        browser.rate(self.settings.script_rate).ok();

        let threw = match browser.run(source, name) {
            Ok(()) => Value::Null,
            Err(error) => json!(error.to_string()),
        };

        browser.rate(1.0).ok();
        browser.running_script("").ok();

        browser.settle(2).ok();

        self.spent(name, started);
        Ok(threw)
    }

    fn spent(&mut self, name: &str, started: std::time::Instant) {
        self.timings.push(json!({
            "name": name,
            "ms": (started.elapsed().as_micros() as f64 / 1000.0).round(),
            "posts": self.sensor_posts(),
        }));
    }

    pub fn eval(&mut self, expression: &str) -> ClientResult<Value> {
        let browser = self
            .browser
            .as_mut()
            .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

        browser
            .eval(expression)
            .map_err(|error| ClientError::internal(error.to_string()))
    }

    fn settle(&mut self) -> ClientResult<()> {
        let wait = self.settings.wait_ms;
        let behaviour = self.settings.behaviour;
        let seed = if self.settings.seed == 0 { now_ms() as u64 } else { self.settings.seed };

        let browser = self
            .browser
            .as_mut()
            .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

        let failed = |error: wre_core::error::Error| {
            ClientError::internal(format!("the sandbox stalled: {error}"))
        };

        browser.load().map_err(failed)?;

        if behaviour {
            self.settle_with_input(seed)?;
        }

        let began = std::time::Instant::now();
        self.wait_until_settled(wait)?;
        self.spent("akamai:validate", began);
        Ok(())
    }

    fn wait_until_settled(&mut self, ms: f64) -> ClientResult<()> {
        self.pump(ms, |session| session.validated())?;

        self.pump(200.0, |session| session.quiet())
    }

    fn landed(&self) -> usize {
        match &self.transport {
            Some(transport) => transport
                .wire
                .sent
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .iter()
                .filter(|sent| !sent.url.contains("/akam/") && sent.bytes > 500 && sent.status > 0)
                .count(),
            None => 0,
        }
    }

    fn quiet(&self) -> bool {
        match &self.transport {
            Some(transport) => transport.pending() == 0,
            None => true,
        }
    }

    fn validated(&self) -> bool {
        self.cookies()
            .abck
            .map(|abck| abck.validated)
            .unwrap_or(false)
    }

    fn settle_with_input(&mut self, seed: u64) -> ClientResult<()> {
        let target = self.field_centre()?;
        let submit = self.submit_centre()?;
        let typed = self.typed_text(seed);

        let began = std::time::Instant::now();
        self.pump(self.settings.load_posts_ms, |session| session.landed() >= 2)?;
        self.spent("akamai:load-posts", began);

        let browser = self
            .browser
            .as_mut()
            .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

        let failed = |error: wre_core::error::Error| {
            ClientError::internal(format!("the sandbox stalled: {error}"))
        };

        browser
            .fire("focus", json!({ "target": "window" }))
            .map_err(failed)?;

        let start = Point::new((target.x - 128.0).max(12.0), (target.y - 52.0).max(12.0));
        let mut stream = Stream::new(seed, start, pointer_shape());
        stream.pause();
        let _ = stream.click_at(target);
        stream.type_text(&typed);

        if let Some(submit) = submit {
            let _ = stream.click_at(submit);
        }

        let before = self.sensor_posts();
        let warp = self.settings.warp.max(1.0);

        let browser = self
            .browser
            .as_mut()
            .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

        browser
            .play_warped(stream.events(), warp)
            .map_err(failed)?;

        let began = std::time::Instant::now();
        self.wait_for_posts(before + 1, self.settings.load_posts_ms)?;
        self.spent("akamai:input-post", began);
        Ok(())
    }

    fn typed_text(&self, seed: u64) -> String {
        if !self.settings.typed.is_empty() {
            return self.settings.typed.clone();
        }

        let alphabet = b"abcdefghijklmnopqrstuvwxyz";
        let mut state = seed | 1;
        let mut text = String::new();

        for _ in 0..(4 + (seed >> 13) % 3) {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            text.push(alphabet[(state >> 33) as usize % alphabet.len()] as char);
        }

        text
    }

    fn submit_centre(&mut self) -> ClientResult<Option<Point>> {
        let answer = self.eval(
            r#"(function () {
  var kinds = ["submit", "button"];
  var fields = [].concat(
    Array.prototype.slice.call(document.getElementsByTagName("button")),
    Array.prototype.slice.call(document.getElementsByTagName("input"))
  );
  for (var index = 0; index < fields.length; index += 1) {
    var field = fields[index];
    if (field.localName === "input" && kinds.indexOf(field.type) === -1) continue;
    if (field.offsetParent === null) continue;
    var box = field.getBoundingClientRect();
    if (!box.width || !box.height) continue;
    if (box.top < 0 || box.top + box.height > window.innerHeight) continue;
    return { x: box.left + box.width / 2, y: box.top + box.height / 2 };
  }
  return null;
})()"#,
        )?;

        let x = answer.get("x").and_then(Value::as_f64);
        let y = answer.get("y").and_then(Value::as_f64);

        Ok(match (x, y) {
            (Some(x), Some(y)) => Some(Point::new(x, y)),
            _ => None,
        })
    }

    fn field_centre(&mut self) -> ClientResult<Point> {
        let answer = self.eval(
            r#"(function () {
  var fields = document.getElementsByTagName("input");
  var chosen = null;

  for (var index = 0; index < fields.length; index += 1) {
    var field = fields[index];
    if (field.type !== "text" && field.type !== "search" && field.type !== "email") continue;
    if (field.offsetParent === null) continue;
    var box = field.getBoundingClientRect();
    if (!box.width || !box.height) continue;

    var form = field.form;
    if (!form) continue;
    if (!form.querySelector('[type="submit"], button')) continue;

    chosen = field;
    break;
  }

  if (!chosen) return null;

  var box = chosen.getBoundingClientRect();
  var wanted = box.top + (window.scrollY || 0) - window.innerHeight / 2 + box.height / 2;
  if (wanted > 0) window.scrollTo(0, wanted);

  box = chosen.getBoundingClientRect();
  return { x: box.left + box.width / 2, y: box.top + box.height / 2 };
})()"#,
        )?;

        let x = answer.get("x").and_then(Value::as_f64);
        let y = answer.get("y").and_then(Value::as_f64);

        match (x, y) {
            (Some(x), Some(y)) => Ok(Point::new(x, y)),
            _ => self.viewport_centre(),
        }
    }

    fn viewport_centre(&mut self) -> ClientResult<Point> {
        let answer = self.eval("({ x: (window.innerWidth || 0) / 2, y: (window.innerHeight || 0) / 2 })")?;

        let x = answer.get("x").and_then(Value::as_f64).unwrap_or_default();
        let y = answer.get("y").and_then(Value::as_f64).unwrap_or_default();

        Ok(Point::new(x.max(16.0), y.max(16.0)))
    }

    fn sensor_posts(&self) -> usize {
        self.requests()
            .iter()
            .filter(|request| !request.url.contains("/akam/"))
            .filter(|request| {
                request
                    .body
                    .as_deref()
                    .map(sensor::looks_like_payload)
                    .unwrap_or(false)
            })
            .count()
    }

    fn wait_for_posts(&mut self, wanted: usize, budget_ms: f64) -> ClientResult<()> {
        self.pump(budget_ms, |session| session.sensor_posts() >= wanted)
    }

    fn pump(&mut self, budget_ms: f64, done: impl Fn(&Self) -> bool) -> ClientResult<()> {
        let slice = 2.0;
        let warp = self.settings.warp.max(1.0);
        let mut spent = 0.0;

        while spent < budget_ms {
            if done(self) {
                return Ok(());
            }

            std::thread::sleep(std::time::Duration::from_millis(slice as u64));

            let browser = self
                .browser
                .as_mut()
                .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

            browser
                .advance(slice * (warp - 1.0))
                .map_err(|error| ClientError::internal(format!("the sandbox stalled: {error}")))?;

            spent += slice;
        }

        Ok(())
    }

    pub fn nudge(&mut self, ms: f64) -> ClientResult<()> {
        let seed = now_ms() as u64;
        let centre = self.viewport_centre()?;
        let from = Point::new((centre.x - 110.0).max(12.0), (centre.y - 30.0).max(12.0));

        let browser = self
            .browser
            .as_mut()
            .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

        let failed = |error: wre_core::error::Error| {
            ClientError::internal(format!("the sandbox stalled: {error}"))
        };

        let mut stream = Stream::new(seed, from, pointer_shape());
        stream.wait(90.0);
        let _ = stream.move_to(centre);
        stream.pause();

        browser.play(stream.events()).map_err(failed)?;
        browser.advance(ms).map_err(failed)?;
        Ok(())
    }

    pub fn telemetry(&mut self) -> ClientResult<Option<String>> {
        let browser = self
            .browser
            .as_mut()
            .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

        let value = browser
            .eval(
                "(typeof bmak === 'object' && bmak && typeof bmak.get_telemetry === 'function') \
                 ? bmak.get_telemetry() : null",
            )
            .map_err(|error| ClientError::internal(format!("get_telemetry failed: {error}")))?;

        Ok(value.as_str().map(str::to_string))
    }

    pub fn payload(&mut self) -> ClientResult<Option<String>> {
        let browser = self
            .browser
            .as_ref()
            .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

        let posted = browser
            .requests()
            .iter()
            .rev()
            .filter_map(|request| request.body.as_deref())
            .filter(|body| sensor::looks_like_payload(body))
            .find_map(sensor::extract);

        if posted.is_some() {
            return Ok(posted);
        }

        if let Some(header) = self.telemetry()?
            && let Some(payload) = sensor::payload_of(&header)
        {
            return Ok(Some(payload));
        }

        Ok(None)
    }

    pub fn endpoint(&self) -> Option<String> {
        let browser = self.browser.as_ref()?;

        let posted = browser
            .requests()
            .iter()
            .rev()
            .find(|request| {
                request
                    .body
                    .as_deref()
                    .map(sensor::looks_like_payload)
                    .unwrap_or(false)
            })
            .map(|request| request.url.clone());

        posted.or_else(|| Some(self.sensor_url.clone()).filter(|url| !url.is_empty()))
    }

    pub fn post_payload(&mut self, payload: &str, endpoint: Option<&str>) -> ClientResult<Sent> {
        let url = match endpoint {
            Some(found) => found.to_string(),
            None => self
                .endpoint()
                .ok_or_else(|| ClientError::bad_input("no endpoint to post the payload to"))?,
        };

        let origin = Url::parse(&self.page_url)
            .ok()
            .map(|parsed| format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or_default()))
            .unwrap_or_default();

        let mut request = FetchRequest::post(url.clone(), sensor::wrap(payload).into_bytes())
            .ordered(&XHR_ORDER)
            .header("accept", "*/*")
            .header("accept-encoding", "gzip, deflate, br, zstd")
            .header("accept-language", self.languages.clone())
            .header("priority", "u=1, i")
            .header("content-type", "text/plain;charset=UTF-8")
            .header("origin", origin)
            .header("referer", self.page_url.clone())
            .header("sec-fetch-dest", "empty")
            .header("sec-fetch-mode", "cors")
            .header("sec-fetch-site", "same-origin")
            .header("user-agent", self.user_agent.clone());

        for (name, value) in &self.hints {
            request = request.header(name.clone(), value.clone());
        }

        let response = self.fetch(request)?;

        let sent = Sent {
            url,
            status: response.status,
            bytes: payload.len(),
            live: true,
            source: "host".to_string(),
            payload: self.settings.keep_payloads.then(|| payload.to_string()),
            abck: self.cookies().abck.map(|abck| abck.status),
            at: now_ms(),
            headers: Vec::new(),
        };

        self.posts.push(sent.clone());
        Ok(sent)
    }

    pub fn run_pixel(&mut self) -> ClientResult<Value> {
        let Some(client) = self.surface.pixel_client.clone() else {
            return Ok(Value::Null);
        };

        let source = if self.loader_source.is_empty() {
            match self.fetch_script(&client.url) {
                Ok(source) => source,
                Err(error) => return Ok(json!({ "url": client.url, "error": error.to_string() })),
            }
        } else {
            self.loader_source.clone()
        };

        if let Some(seed) = self.surface.baza.clone() {
            let browser = self
                .browser
                .as_mut()
                .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

            browser
                .run(&format!("globalThis.bazadebezolkohpepadr = {};", json!(seed)), "akamai:baza")
                .map_err(|error| ClientError::internal(format!("the seed did not take: {error}")))?;
        }

        let threw = self.run(&source, "akamai:pixel")?;

        self.run(
            r#"(function () {
  var nodes = document.getElementsByTagName("script");
  for (var index = nodes.length - 1; index >= 0; index -= 1) {
    var node = nodes[index];
    var src = node.getAttribute ? node.getAttribute("src") : "";
    var text = String(node.text || node.textContent || "");
    var mine = (src && src.indexOf("/akam/") !== -1) || text.indexOf("bazadebezolkohpepadr") !== -1;
    if (mine && node.parentNode) node.parentNode.removeChild(node);
  }
  try { delete window.bazadebezolkohpepadr; } catch (error) {}
})()"#,
            "akamai:pixel-cleanup",
        )?;

        let browser = self
            .browser
            .as_mut()
            .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

        browser
            .advance(1200.0)
            .map_err(|error| ClientError::internal(format!("the sandbox stalled: {error}")))?;

        let posted: Vec<Request> = browser
            .requests()
            .into_iter()
            .filter(|request| request.url.contains("/pixel_"))
            .collect();

        let mut sent = Vec::new();

        if !self.settings.live_xhr {
            for request in &posted {
                sent.push(self.replay(request)?);
            }
        }

        Ok(json!({
            "url": client.url,
            "post": self.surface.pixel_post,
            "bytes": source.len(),
            "posts": posted.len(),
            "sent": sent,
            "threw": threw,
        }))
    }

    pub fn replay(&mut self, request: &Request) -> ClientResult<Sent> {
        let origin = Url::parse(&self.page_url)
            .ok()
            .map(|parsed| format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or_default()))
            .unwrap_or_default();

        let mut outgoing = FetchRequest {
            url: request.url.clone(),
            method: request.method.to_uppercase(),
            headers: Vec::new(),
            body: request.body.as_ref().map(|body| body.as_bytes().to_vec()),
            fingerprint: None,
            order: XHR_ORDER.iter().map(|name| name.to_string()).collect(),
        };

        let mut headers: BTreeMap<String, String> = BTreeMap::from([
            ("accept".to_string(), "*/*".to_string()),
            ("accept-encoding".to_string(), "gzip, deflate, br, zstd".to_string()),
            ("accept-language".to_string(), self.languages.clone()),
            ("origin".to_string(), origin),
            ("priority".to_string(), "u=1, i".to_string()),
            ("referer".to_string(), self.page_url.clone()),
            ("sec-fetch-dest".to_string(), "empty".to_string()),
            ("sec-fetch-mode".to_string(), "cors".to_string()),
            ("sec-fetch-site".to_string(), "same-origin".to_string()),
            ("user-agent".to_string(), self.user_agent.clone()),
        ]);

        headers.extend(self.hints.iter().cloned());

        for (name, value) in &request.headers {
            headers.insert(name.to_lowercase(), value.clone());
        }

        outgoing.headers = headers.into_iter().collect();

        let response = self.fetch(outgoing)?;
        let sent = Sent {
            url: request.url.clone(),
            status: response.status,
            bytes: request.body.as_ref().map(String::len).unwrap_or_default(),
            live: true,
            abck: self.cookies().abck.map(|abck| abck.status),
            at: now_ms(),
            headers: Vec::new(),
            source: format!("replay:{}", request.source),
            payload: self
                .settings
                .keep_payloads
                .then(|| {
                    request
                        .body
                        .as_deref()
                        .and_then(|body| sensor::extract(body).or_else(|| sensor::payload_of(body)))
                })
                .flatten(),
        };

        self.posts.push(sent.clone());
        Ok(sent)
    }

    pub fn sensor_source(&self) -> (&str, &str) {
        (&self.sensor_url, &self.sensor_source)
    }

    pub fn taken(&self) -> Vec<Sent> {
        match &self.transport {
            Some(transport) => {
                let mut sent = transport.wire.sent.lock().unwrap_or_else(|error| error.into_inner());
                std::mem::take(&mut *sent)
            }
            None => Vec::new(),
        }
    }

    pub fn posts(&self) -> &[Sent] {
        &self.posts
    }

    pub fn requests(&self) -> Vec<Request> {
        self.browser.as_ref().map(Browser::requests).unwrap_or_default()
    }

    pub fn misses(&self) -> Vec<String> {
        self.browser.as_ref().map(Browser::misses).unwrap_or_default()
    }

    pub fn start_ts(&mut self) -> ClientResult<Option<u64>> {
        let browser = self
            .browser
            .as_mut()
            .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

        let value = browser
            .eval("(typeof bmak === 'object' && bmak) ? bmak.startTs : null")
            .map_err(|error| ClientError::internal(format!("startTs failed: {error}")))?;

        Ok(value.as_f64().map(|found| found as u64))
    }

    pub fn close(&mut self) {
        self.browser = None;
        self.transport = None;
    }
}
