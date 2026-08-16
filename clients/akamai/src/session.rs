use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use wre_behavior::stream::{Point, Stream};
use wre_client::context::{FetchRequest, FetchResponse, Http, Jar};
use wre_client::error::{ClientError, ClientResult};
use wre_live::realm::RealmOptions;
use wre_sandbox::browser::{Answer, Browser, CookieStore, Hooks, Request, Transport, now_ms, open};
use wre_sandbox::page::Page;
use wre_sandbox::profile::Profile;

use crate::cookies::{self, Summary};
use crate::discover::{Surface, discover};
use crate::sensor;

const NAVIGATE: [(&str, &str); 6] = [
    (
        "accept",
        "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8",
    ),
    ("accept-language", "en-US,en;q=0.9"),
    ("sec-fetch-dest", "document"),
    ("sec-fetch-mode", "navigate"),
    ("sec-fetch-site", "none"),
    ("upgrade-insecure-requests", "1"),
];

const SCRIPT: [(&str, &str); 5] = [
    ("accept", "*/*"),
    ("accept-language", "en-US,en;q=0.9"),
    ("sec-fetch-dest", "script"),
    ("sec-fetch-mode", "no-cors"),
    ("sec-fetch-site", "same-origin"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub wait_ms: f64,
    pub init_cost_ms: f64,
    pub friction_ms: f64,
    pub behaviour: bool,
    pub paced: bool,
    pub pixel: bool,
    pub live_xhr: bool,
    pub timeout_ms: u64,
    pub seed: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            wait_ms: 20_000.0,
            init_cost_ms: 25.0,
            friction_ms: 0.12,
            behaviour: true,
            paced: true,
            pixel: true,
            live_xhr: false,
            timeout_ms: 90_000,
            seed: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sent {
    pub url: String,
    pub status: u16,
    pub bytes: usize,
    pub live: bool,
    pub source: String,
}

struct Live {
    http: Arc<Http>,
    origin: String,
    referer: String,
    user_agent: String,
    enabled: bool,
    sent: Mutex<Vec<Sent>>,
}

impl Live {
    fn same_origin(&self, url: &str) -> bool {
        match (Url::parse(url), Url::parse(&self.origin)) {
            (Ok(target), Ok(origin)) => target.host_str() == origin.host_str(),
            _ => false,
        }
    }
}

impl Transport for Live {
    fn send(&self, request: &Request) -> Answer {
        let bytes = request.body.as_ref().map(String::len).unwrap_or_default();

        if !self.enabled || !self.same_origin(&request.url) {
            let mut sent = self.sent.lock().unwrap_or_else(|error| error.into_inner());
            sent.push(Sent {
                url: request.url.clone(),
                status: 0,
                bytes,
                live: false,
                source: request.source.clone(),
            });
            return Answer::default();
        }

        let mut outgoing = FetchRequest {
            url: request.url.clone(),
            method: request.method.to_uppercase(),
            headers: Vec::new(),
            body: request.body.as_ref().map(|body| body.as_bytes().to_vec()),
            fingerprint: None,
        };

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

        for (name, value) in &request.headers {
            headers.insert(name.to_lowercase(), value.clone());
        }

        outgoing.headers = headers.into_iter().collect();

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
            },
            Err(_) => Answer { status: 0, body: String::new(), headers: Vec::new() },
        };

        let mut sent = self.sent.lock().unwrap_or_else(|error| error.into_inner());
        sent.push(Sent {
            url: request.url.clone(),
            status: answer.status,
            bytes,
            live: true,
            source: request.source.clone(),
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
    page_url: String,
    html: String,
    surface: Surface,
    sensor_url: String,
    sensor_bytes: usize,
    browser: Option<Browser>,
    transport: Option<Arc<Live>>,
    posts: Vec<Sent>,
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
        Self {
            http,
            jar,
            profile,
            profile_id,
            settings,
            user_agent,
            page_url: String::new(),
            html: String::new(),
            surface: Surface::default(),
            sensor_url: String::new(),
            sensor_bytes: 0,
            browser: None,
            transport: None,
            posts: Vec::new(),
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
        self.http.fetch(request)
    }

    pub fn navigate(&mut self, url: &str) -> ClientResult<FetchResponse> {
        let mut request = FetchRequest::get(url).header("user-agent", self.user_agent.clone());
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
            .header("referer", self.page_url.clone());

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

        let transport = Arc::new(Live {
            http: Arc::clone(&self.http),
            origin,
            referer: self.page_url.clone(),
            user_agent: self.user_agent.clone(),
            enabled: self.settings.live_xhr,
            sent: Mutex::new(Vec::new()),
        });

        let hooks = Hooks {
            transport: Arc::clone(&transport) as Arc<dyn Transport>,
            cookies: Arc::new(Cookies { jar: self.jar.clone(), url: self.page_url.clone() }),
        };

        let mut page = Page::read(&self.page_url, &self.html)
            .with_epoch(now_ms())
            .with_friction(self.settings.friction_ms);

        if !self.sensor_url.is_empty() {
            page = page.with_script(&self.sensor_url);
        }

        let options = RealmOptions {
            timeout: std::time::Duration::from_millis(self.settings.timeout_ms.max(1000)),
            timers: false,
            codecs: true,
            clock_ms: None,
            random_seed: None,
            heap_limit_mb: None,
        };

        let mut browser = open(&self.profile, &page, hooks, options)
            .map_err(|error| ClientError::internal(format!("the sandbox did not open: {error}")))?;

        browser
            .charge_on("bmak", "startTs", self.settings.init_cost_ms)
            .map_err(|error| ClientError::internal(format!("the clock charge failed: {error}")))?;

        self.browser = Some(browser);
        self.transport = Some(transport);
        Ok(())
    }

    pub fn open(&mut self, url: &str) -> ClientResult<Value> {
        let page = self.navigate(url)?;

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

        let source = self.fetch_script(&sensor.url)?;

        if source.len() < 1000 {
            return Err(ClientError::resource(format!(
                "the sensor script is only {} bytes, that is not a build",
                source.len()
            )));
        }

        self.sensor_url = sensor.url.clone();
        self.sensor_bytes = source.len();
        self.mount()?;

        let threw = self.run(&source, "akamai:sensor")?;
        self.settle()?;

        let pixel = if self.settings.pixel { self.run_pixel()? } else { Value::Null };

        let browser = self.browser.as_mut().expect("mounted");
        let misses = browser.misses();
        let requests = browser.requests().len();

        let sent = self.taken();
        self.posts.extend(sent.iter().cloned());

        Ok(json!({
            "page": { "url": self.page_url, "status": page.status, "bytes": self.html.len() },
            "sensor": { "url": self.sensor_url, "bytes": self.sensor_bytes },
            "profile": self.profile_id,
            "surface": self.surface,
            "pixel": pixel,
            "threw": threw,
            "requests": requests,
            "sent": sent,
            "misses": misses,
            "cookies": self.cookies(),
        }))
    }

    fn run(&mut self, source: &str, name: &str) -> ClientResult<Value> {
        let browser = self
            .browser
            .as_mut()
            .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

        match browser.run(source, name) {
            Ok(()) => Ok(Value::Null),
            Err(error) => Ok(json!(error.to_string())),
        }
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
        browser.advance(250.0).map_err(failed)?;
        std::thread::sleep(std::time::Duration::from_millis(250));

        if behaviour {
            let mut stream = Stream::seeded(seed);
            stream.wait(120.0);
            let _ = stream.move_to(Point::new(412.0, 308.0));
            stream.pause();
            let _ = stream.move_to(Point::new(640.0, 420.0));
            stream.click();
            stream.type_text("ab");
            stream.scroll_by(0.0, 240.0, 6);

            browser.play(stream.events()).map_err(failed)?;
        }

        self.wait(wait)?;
        Ok(())
    }

    pub fn wait(&mut self, ms: f64) -> ClientResult<()> {
        let paced = self.settings.paced;

        let browser = self
            .browser
            .as_mut()
            .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

        let failed = |error: wre_core::error::Error| {
            ClientError::internal(format!("the sandbox stalled: {error}"))
        };

        if !paced {
            browser.advance(ms).map_err(failed)?;
            return Ok(());
        }

        let step = 250.0;
        let mut left = ms;

        while left > 0.0 {
            let slice = left.min(step);
            browser.advance(slice).map_err(failed)?;
            std::thread::sleep(std::time::Duration::from_millis(slice as u64));
            left -= slice;
        }

        Ok(())
    }

    pub fn nudge(&mut self, ms: f64) -> ClientResult<()> {
        let seed = now_ms() as u64;
        let browser = self
            .browser
            .as_mut()
            .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

        let failed = |error: wre_core::error::Error| {
            ClientError::internal(format!("the sandbox stalled: {error}"))
        };

        let mut stream = Stream::seeded(seed);
        stream.wait(90.0);
        let _ = stream.move_to(Point::new(520.0, 360.0));
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
        if let Some(header) = self.telemetry()?
            && let Some(payload) = sensor::payload_of(&header)
        {
            return Ok(Some(payload));
        }

        let browser = self
            .browser
            .as_ref()
            .ok_or_else(|| ClientError::internal("the sandbox is not mounted"))?;

        Ok(browser
            .requests()
            .iter()
            .rev()
            .filter_map(|request| request.body.as_deref())
            .filter(|body| sensor::looks_like_payload(body))
            .find_map(sensor::extract))
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

        let request = FetchRequest::post(url.clone(), sensor::wrap(payload).into_bytes())
            .header("accept", "*/*")
            .header("accept-language", "en-US,en;q=0.9")
            .header("content-type", "application/json")
            .header("origin", origin)
            .header("referer", self.page_url.clone())
            .header("sec-fetch-dest", "empty")
            .header("sec-fetch-mode", "cors")
            .header("sec-fetch-site", "same-origin")
            .header("user-agent", self.user_agent.clone());

        let response = self.fetch(request)?;

        let sent = Sent {
            url,
            status: response.status,
            bytes: payload.len(),
            live: true,
            source: "host".to_string(),
        };

        self.posts.push(sent.clone());
        Ok(sent)
    }

    pub fn run_pixel(&mut self) -> ClientResult<Value> {
        let Some(client) = self.surface.pixel_client.clone() else {
            return Ok(Value::Null);
        };

        let source = match self.fetch_script(&client.url) {
            Ok(source) => source,
            Err(error) => return Ok(json!({ "url": client.url, "error": error.to_string() })),
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
        };

        let mut headers: BTreeMap<String, String> = BTreeMap::from([
            ("accept".to_string(), "*/*".to_string()),
            ("accept-language".to_string(), "en-US,en;q=0.9".to_string()),
            ("origin".to_string(), origin),
            ("referer".to_string(), self.page_url.clone()),
            ("sec-fetch-dest".to_string(), "empty".to_string()),
            ("sec-fetch-mode".to_string(), "cors".to_string()),
            ("sec-fetch-site".to_string(), "same-origin".to_string()),
            ("user-agent".to_string(), self.user_agent.clone()),
        ]);

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
            source: format!("replay:{}", request.source),
        };

        self.posts.push(sent.clone());
        Ok(sent)
    }

    pub fn taken(&self) -> Vec<Sent> {
        match &self.transport {
            Some(transport) => {
                let mut sent = transport.sent.lock().unwrap_or_else(|error| error.into_inner());
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
