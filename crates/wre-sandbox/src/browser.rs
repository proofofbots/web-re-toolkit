use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use wre_behavior::stream::Event as Motion;
use wre_core::error::{Error, Result};
use wre_live::realm::{Control, Realm, RealmOptions};

use crate::install::{Misses, Sandbox, install};
use crate::page::Page;
use crate::profile::Profile;

pub const BROWSER: &str = include_str!("../assets/browser.js");

const BLANK_PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

const DRAIN_ROUNDS: usize = 64;
const TIMER_STEPS: usize = 20000;
const MIN_DRAIN_ROUNDS: usize = 6;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Request {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default, rename = "bodyBytes", skip_serializing_if = "Option::is_none")]
    pub body_base64: Option<String>,
    #[serde(default)]
    pub at: f64,
    #[serde(default)]
    pub source: String,
}

impl Request {
    pub fn bytes(&self) -> Option<Vec<u8>> {
        if let Some(encoded) = &self.body_base64 {
            return STANDARD.decode(encoded).ok();
        }

        self.body.as_ref().map(|text| text.as_bytes().to_vec())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
    pub status: u16,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub paced: f64,
}

impl Answer {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

impl Default for Answer {
    fn default() -> Self {
        Self {
            status: 201,
            body: r#"{"success":true}"#.to_string(),
            headers: Vec::new(),
            paced: 0.0,
        }
    }
}

pub trait Transport: Send + Sync {
    fn send(&self, request: &Request) -> Answer;

    fn start(&self, _request: &Request) -> Option<u64> {
        None
    }

    fn take(&self, _ticket: u64) -> Option<Answer> {
        None
    }
}

pub trait CookieStore: Send + Sync {
    fn read(&self) -> String;
    fn write(&self, assignment: &str);
}

#[derive(Debug, Default)]
pub struct Offline;

impl Transport for Offline {
    fn send(&self, _request: &Request) -> Answer {
        Answer::default()
    }
}

#[derive(Debug, Default)]
pub struct Held {
    values: Mutex<Vec<(String, String)>>,
}

impl Held {
    pub fn seeded(header: &str) -> Self {
        let held = Self::default();
        for pair in header.split(';') {
            let trimmed = pair.trim();
            if let Some((name, value)) = trimmed.split_once('=') {
                held.write(&format!("{name}={value}"));
            }
        }
        held
    }
}

impl CookieStore for Held {
    fn read(&self) -> String {
        let values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        values
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn write(&self, assignment: &str) {
        let head = assignment.split(';').next().unwrap_or_default().trim();
        let Some((name, value)) = head.split_once('=') else {
            return;
        };

        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());

        match values.iter_mut().find(|(known, _)| known == name.trim()) {
            Some(entry) => entry.1 = value.to_string(),
            None => values.push((name.trim().to_string(), value.to_string())),
        }
    }
}

pub struct Hooks {
    pub transport: Arc<dyn Transport>,
    pub cookies: Arc<dyn CookieStore>,
}

impl Default for Hooks {
    fn default() -> Self {
        Self {
            transport: Arc::new(Offline),
            cookies: Arc::new(Held::default()),
        }
    }
}

pub struct Browser {
    realm: Realm,
    control: Control,
    sandbox: Sandbox,
    misses: Misses,
    requests: Arc<Mutex<Vec<Request>>>,
    heap: Arc<Mutex<(u64, u64)>>,
    screen: (f64, f64),
}

pub fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as f64)
        .unwrap_or_default()
}

pub fn open(
    profile: &Profile,
    page: &Page,
    hooks: Hooks,
    options: RealmOptions,
) -> Result<Browser> {
    let mut realm = Realm::new(RealmOptions {
        timers: false,
        ..options
    })?;
    let sandbox = install(&mut realm, profile)?;
    let misses = sandbox.misses_handle();
    let requests = Arc::new(Mutex::new(Vec::new()));

    realm.install_source_mask()?;

    let blob = serde_json::to_value(profile)
        .map_err(|error| Error::msg(format!("the profile did not serialise: {error}")))?;
    realm.register_host("__wreProfileBlob", Box::new(move |_args| Ok(blob.clone())))?;

    let epoch = if page.epoch_ms > 0.0 {
        page.epoch_ms
    } else {
        now_ms()
    };
    let described = page.describe()?;
    let mut described = described;
    described["epoch"] = json!(epoch);
    realm.register_host(
        "__wrePageBlob",
        Box::new(move |_args| Ok(described.clone())),
    )?;

    realm.register_host(
        "__wreRealNow",
        Box::new(|_args| {
            let precise = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_secs_f64() * 1000.0)
                .unwrap_or_default();

            Ok(json!(precise))
        }),
    )?;

    let transport = Arc::clone(&hooks.transport);
    let recorded = Arc::clone(&requests);

    realm.register_host(
        "__wreRequest",
        Box::new(move |args| {
            let raw = args.first().cloned().unwrap_or(Value::Null);
            let request: Request = serde_json::from_value(raw)
                .map_err(|error| Error::msg(format!("the sandbox sent a bad request: {error}")))?;

            {
                let mut list = recorded.lock().unwrap_or_else(|error| error.into_inner());
                list.push(request.clone());
            }

            let answer = transport.send(&request);
            serde_json::to_value(answer)
                .map_err(|error| Error::msg(format!("the answer did not serialise: {error}")))
        }),
    )?;

    let transport = Arc::clone(&hooks.transport);
    let recorded = Arc::clone(&requests);

    realm.register_host(
        "__wreRequestStart",
        Box::new(move |args| {
            let raw = args.first().cloned().unwrap_or(Value::Null);
            let request: Request = serde_json::from_value(raw)
                .map_err(|error| Error::msg(format!("the sandbox sent a bad request: {error}")))?;

            let Some(ticket) = transport.start(&request) else {
                return Ok(Value::Null);
            };

            {
                let mut list = recorded.lock().unwrap_or_else(|error| error.into_inner());
                list.push(request);
            }

            Ok(json!(ticket))
        }),
    )?;

    let transport = Arc::clone(&hooks.transport);

    realm.register_host(
        "__wreRequestTake",
        Box::new(move |args| {
            let ticket = args.first().and_then(Value::as_u64).unwrap_or_default();

            match transport.take(ticket) {
                Some(answer) => serde_json::to_value(answer)
                    .map_err(|error| Error::msg(format!("the answer did not serialise: {error}"))),
                None => Ok(Value::Null),
            }
        }),
    )?;

    let reader = Arc::clone(&hooks.cookies);
    realm.register_host(
        "__wreCookieRead",
        Box::new(move |_args| Ok(json!(reader.read()))),
    )?;

    let writer = Arc::clone(&hooks.cookies);
    realm.register_host(
        "__wreCookieWrite",
        Box::new(move |args| {
            if let Some(text) = args.first().and_then(Value::as_str) {
                writer.write(text);
            }
            Ok(Value::Null)
        }),
    )?;

    let images = profile.canvas.clone();
    let watcher = misses.clone();

    realm.register_host(
        "__wreCanvasImage",
        Box::new(move |args| {
            let key = args
                .first()
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            if let Some(found) = images.get(&key) {
                return Ok(json!(found));
            }

            let ops = args
                .get(4)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .chars()
                .take(600)
                .collect::<String>();

            watcher.record(&format!("canvas toDataURL({key}) {ops}"));

            match images.get("default") {
                Some(found) => Ok(json!(found)),
                None => Ok(json!(BLANK_PNG)),
            }
        }),
    )?;

    let widths = profile.font_widths.clone();
    let probe = profile.font_probe.clone();
    let watcher = misses.clone();

    realm.register_host(
        "__wreMeasureText",
        Box::new(move |args| {
            let font = args.first().and_then(Value::as_str).unwrap_or_default();
            let text = args.get(1).and_then(Value::as_str).unwrap_or_default();

            Ok(json!(measure(&widths, &probe, font, text, &watcher)))
        }),
    )?;

    let watcher = misses.clone();
    realm.register_host(
        "__wreMiss",
        Box::new(move |args| {
            if let Some(text) = args.first().and_then(Value::as_str) {
                watcher.record(text);
            }
            Ok(Value::Null)
        }),
    )?;

    let heap = Arc::new(Mutex::new((0_u64, 0_u64)));
    let reader = Arc::clone(&heap);

    realm.register_host(
        "__wreHeap",
        Box::new(move |_args| {
            let held = reader.lock().unwrap_or_else(|error| error.into_inner());
            Ok(json!([held.0, held.1]))
        }),
    )?;

    crate::host::install(&mut realm)?;

    let control = realm.attach(BROWSER, "wre:browser")?;

    let names = realm.invoke(&control, "masked", &[])?;
    if let Some(list) = names.as_array() {
        for entry in list {
            let Some(expression) = entry.as_str() else {
                continue;
            };
            realm.mask_sources(expression).ok();
            realm.mask_source(expression).ok();
        }
    }

    let mut browser = Browser {
        realm,
        control,
        sandbox,
        misses,
        requests,
        heap,
        screen: screen_offset(profile),
    };

    browser.measure_heap();
    Ok(browser)
}

fn measure(
    widths: &BTreeMap<String, f64>,
    probe: &str,
    font: &str,
    text: &str,
    misses: &Misses,
) -> f64 {
    let (probe_size, probe_text) = split_probe(probe);
    let (size, family) = split_font(font);

    let per_character = match widths.get(&family) {
        Some(width) => width / probe_text.chars().count().max(1) as f64,
        None => {
            misses.record(&format!("measureText font {family}"));
            match widths.values().next() {
                Some(width) => width / probe_text.chars().count().max(1) as f64,
                None => probe_size * 0.5,
            }
        }
    };

    let scale = if probe_size > 0.0 {
        size / probe_size
    } else {
        1.0
    };
    let width = per_character * scale * text.chars().count() as f64;

    (width * 1000.0).round() / 1000.0
}

fn split_probe(probe: &str) -> (f64, String) {
    let mut parts = probe.splitn(2, ' ');
    let size = parts.next().unwrap_or("72px");
    let text = parts.next().unwrap_or("mmmmmmmmmmlli");

    (pixels(size).unwrap_or(72.0), text.to_string())
}

fn split_font(font: &str) -> (f64, String) {
    let mut size = 10.0;
    let mut family = String::new();

    for part in font.split_whitespace() {
        if let Some(found) = pixels(part) {
            size = found;
            continue;
        }
        if !family.is_empty() {
            family.push(' ');
        }
        family.push_str(part);
    }

    let family = family
        .split(',')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches(['"', '\''])
        .to_string();

    (size, family)
}

fn pixels(text: &str) -> Option<f64> {
    let trimmed = text.trim();

    if let Some(number) = trimmed.strip_suffix("px") {
        return number.parse::<f64>().ok();
    }
    if let Some(number) = trimmed.strip_suffix("pt") {
        return number.parse::<f64>().ok().map(|value| value * 4.0 / 3.0);
    }

    None
}

impl Browser {
    pub fn realm(&mut self) -> &mut Realm {
        &mut self.realm
    }

    pub fn installed(&self) -> &[String] {
        self.sandbox.installed()
    }

    pub fn misses(&self) -> Vec<String> {
        self.misses.all()
    }

    pub fn errors(&mut self) -> Vec<String> {
        let records = match self.realm.records() {
            Ok(found) => found,
            Err(_) => return Vec::new(),
        };

        records
            .errors
            .into_iter()
            .map(|entry| match entry.where_ {
                Some(place) => format!("{place}: {}", entry.text),
                None => entry.text,
            })
            .collect()
    }

    pub fn requests(&self) -> Vec<Request> {
        let list = self
            .requests
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        list.clone()
    }

    pub fn run(&mut self, source: &str, name: &str) -> Result<()> {
        self.realm.eval_unit(source, name)
    }

    pub fn eval(&mut self, expression: &str) -> Result<Value> {
        self.realm.eval_json(expression)
    }

    fn measure_heap(&mut self) {
        let (total, used) = self.realm.heap();
        let mut held = self.heap.lock().unwrap_or_else(|error| error.into_inner());
        *held = (total as u64, used as u64);
    }

    pub fn advance(&mut self, ms: f64) -> Result<usize> {
        self.measure_heap();

        let until = self
            .realm
            .invoke(&self.control, "horizon", &[json!(ms)])?
            .as_f64()
            .unwrap_or_default();

        let mut total = 0;

        for _ in 0..TIMER_STEPS {
            self.realm.pump_microtasks();

            let ran = self
                .realm
                .invoke(&self.control, "stepTo", &[json!(until)])?
                .as_u64()
                .unwrap_or_default() as usize;

            if ran == 0 {
                break;
            }

            total += ran;
        }

        self.realm.invoke(&self.control, "reach", &[json!(until)])?;
        total += self.drain()?;
        Ok(total)
    }

    pub fn settle(&mut self, rounds: usize) -> Result<usize> {
        self.measure_heap();

        let ran = self
            .realm
            .invoke(&self.control, "settle", &[json!(rounds)])?;
        let mut total = ran.as_u64().unwrap_or_default() as usize;
        total += self.drain()?;
        Ok(total)
    }

    fn drain(&mut self) -> Result<usize> {
        let mut total = 0;
        let mut quiet = 0;

        for round in 0..DRAIN_ROUNDS {
            self.realm.pump_microtasks();

            let ran = self
                .realm
                .invoke(&self.control, "settle", &[json!(1)])?
                .as_u64()
                .unwrap_or_default() as usize;

            total += ran;
            quiet = if ran == 0 { quiet + 1 } else { 0 };

            if round >= MIN_DRAIN_ROUNDS && quiet >= 2 {
                break;
            }
        }

        Ok(total)
    }

    pub fn fire(&mut self, kind: &str, detail: Value) -> Result<()> {
        self.realm
            .invoke(&self.control, "fire", &[json!(kind), detail])?;
        Ok(())
    }

    pub fn ready(&mut self, state: &str) -> Result<()> {
        self.realm.invoke(&self.control, "ready", &[json!(state)])?;
        Ok(())
    }

    pub fn now(&mut self) -> Result<f64> {
        let value = self.realm.invoke(&self.control, "now", &[])?;
        Ok(value.as_f64().unwrap_or_default())
    }

    pub fn elapsed(&mut self) -> Result<f64> {
        let value = self.realm.invoke(&self.control, "elapsed", &[])?;
        Ok(value.as_f64().unwrap_or_default())
    }

    pub fn pending(&mut self) -> Result<usize> {
        let value = self.realm.invoke(&self.control, "pending", &[])?;
        Ok(value.as_u64().unwrap_or_default() as usize)
    }

    pub fn jump(&mut self, ms: f64) -> Result<()> {
        self.realm.invoke(&self.control, "jump", &[json!(ms)])?;
        Ok(())
    }

    pub fn charge_on(&mut self, global: &str, property: &str, ms: f64) -> Result<()> {
        self.realm.invoke(
            &self.control,
            "chargeOn",
            &[json!(global), json!(property), json!(ms)],
        )?;
        Ok(())
    }

    pub fn running_script(&mut self, src: &str) -> Result<()> {
        self.realm.invoke(&self.control, "runningScript", &[json!(src)])?;
        Ok(())
    }

    pub fn rate(&mut self, factor: f64) -> Result<()> {
        self.realm.invoke(&self.control, "rate", &[json!(factor)])?;
        Ok(())
    }

    pub fn friction(&mut self, ms: f64) -> Result<()> {
        self.realm.invoke(&self.control, "friction", &[json!(ms)])?;
        Ok(())
    }

    pub fn cookies(&mut self) -> Result<String> {
        let value = self.realm.invoke(&self.control, "cookies", &[])?;
        Ok(value.as_str().unwrap_or_default().to_string())
    }

    pub fn finish_parse(&mut self) -> Result<()> {
        self.realm.invoke(&self.control, "parsed", &[])?;
        Ok(())
    }

    pub fn load(&mut self) -> Result<()> {
        self.ready("interactive")?;
        self.settle(4)?;
        self.ready("complete")?;
        self.settle(4)?;
        Ok(())
    }

    pub fn play(&mut self, events: &[Motion]) -> Result<usize> {
        self.play_warped(events, f64::INFINITY)
    }

    pub fn play_warped(&mut self, events: &[Motion], warp: f64) -> Result<usize> {
        let mut clock = 0.0;
        let mut fired = 0;

        for event in events {
            let gap = event.at() - clock;
            if gap > 0.0 {
                if warp.is_finite() && warp > 0.0 {
                    let real = (gap / warp).min(2_000.0);
                    if real >= 1.0 {
                        std::thread::sleep(std::time::Duration::from_millis(real as u64));
                        self.advance(0.0)?;
                    }
                }

                self.advance(gap)?;
                clock = event.at();
            }

            for (kind, detail) in expand(event, self.screen) {
                self.fire(&kind, detail)?;
                fired += 1;
            }
        }

        Ok(fired)
    }
}

fn screen_offset(profile: &Profile) -> (f64, f64) {
    let read = |name: &str| {
        profile
            .property("Window", name)
            .and_then(Value::as_f64)
            .unwrap_or_default()
    };

    let chrome = (read("outerHeight") - read("innerHeight")).max(0.0);

    (read("screenX"), read("screenY") + chrome)
}

fn expand(event: &Motion, screen: (f64, f64)) -> Vec<(String, Value)> {
    let point = event.position();
    let base = point
        .map(|point| {
            json!({
                "clientX": point.x.round(),
                "clientY": point.y.round(),
                "pageX": point.x.round(),
                "pageY": point.y.round(),
                "screenX": (point.x + screen.0).round(),
                "screenY": (point.y + screen.1).round(),
            })
        })
        .unwrap_or_else(|| json!({}));

    let with = |extra: Value| -> Value {
        let mut merged = base.clone();
        if let (Some(target), Some(source)) = (merged.as_object_mut(), extra.as_object()) {
            for (key, value) in source {
                target.insert(key.clone(), value.clone());
            }
        }
        merged
    };

    match event {
        Motion::PointerMove { .. } => vec![
            (
                "pointermove".to_string(),
                with(json!({ "pointerType": "mouse", "isPrimary": true })),
            ),
            ("mousemove".to_string(), with(json!({ "buttons": 0 }))),
        ],
        Motion::PointerDown { button, .. } => vec![
            (
                "pointerdown".to_string(),
                with(
                    json!({ "pointerType": "mouse", "isPrimary": true, "button": button, "buttons": 1, "pressure": 0.5 }),
                ),
            ),
            (
                "mousedown".to_string(),
                with(json!({ "button": button, "buttons": 1 })),
            ),
        ],
        Motion::PointerUp { button, .. } => vec![
            (
                "pointerup".to_string(),
                with(
                    json!({ "pointerType": "mouse", "isPrimary": true, "button": button, "buttons": 0 }),
                ),
            ),
            (
                "mouseup".to_string(),
                with(json!({ "button": button, "buttons": 0 })),
            ),
            (
                "click".to_string(),
                with(json!({ "button": button, "detail": 1 })),
            ),
        ],
        Motion::TouchStart { .. } => vec![("touchstart".to_string(), with(json!({})))],
        Motion::TouchMove { .. } => vec![("touchmove".to_string(), with(json!({})))],
        Motion::TouchEnd { .. } => vec![("touchend".to_string(), with(json!({})))],
        Motion::KeyDown { key, .. } => {
            let mut out = vec![(
                "keydown".to_string(),
                json!({ "key": key, "code": code_for(key), "keyCode": key_code(key), "which": key_code(key) }),
            )];

            if let Some(character) = printable(key) {
                out.push((
                    "keypress".to_string(),
                    json!({ "key": key, "code": code_for(key), "charCode": character }),
                ));
            }

            out
        }
        Motion::KeyUp { key, .. } => vec![(
            "keyup".to_string(),
            json!({ "key": key, "code": code_for(key), "keyCode": key_code(key), "which": key_code(key) }),
        )],
        Motion::Scroll { .. } => vec![("scroll".to_string(), json!({ "target": "window" }))],
    }
}

fn printable(key: &str) -> Option<u32> {
    let mut characters = key.chars();
    let first = characters.next()?;

    if characters.next().is_some() {
        return None;
    }

    Some(first as u32)
}

fn key_code(key: &str) -> u32 {
    let Some(first) = key.chars().next() else {
        return 0;
    };

    match key {
        " " => 32,
        "Enter" => 13,
        "Tab" => 9,
        "Backspace" => 8,
        "Shift" => 16,
        _ => first.to_ascii_uppercase() as u32,
    }
}

fn code_for(key: &str) -> String {
    let Some(first) = key.chars().next() else {
        return String::new();
    };

    if first.is_ascii_alphabetic() {
        return format!("Key{}", first.to_ascii_uppercase());
    }
    if first.is_ascii_digit() {
        return format!("Digit{first}");
    }

    match key {
        " " => "Space".to_string(),
        "Enter" => "Enter".to_string(),
        "Tab" => "Tab".to_string(),
        "Backspace" => "Backspace".to_string(),
        other => other.to_string(),
    }
}
