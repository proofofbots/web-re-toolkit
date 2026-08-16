use std::sync::{Arc, Mutex};
use std::time::Instant;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use wre_core::error::{Error, Result};
use wre_live::realm::{Control, Realm, RealmOptions};

use crate::browser::{Hooks, Request};
use crate::install::Misses;

const BRIDGE: &str = include_str!("../assets/graph/bridge.js");
const BOOTSTRAP: &str = include_str!("../assets/graph/bootstrap.js");
const DOM: &str = include_str!("../assets/graph/dom.js");
const CONTROL: &str = include_str!("../assets/graph/control.js");

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tables {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traits: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shapes: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webgl: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graphics: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<Value>,
}

fn zero_box() -> Value {
    json!({
        "clientWidth": 0,
        "clientHeight": 0,
        "scrollWidth": 0,
        "scrollHeight": 0,
        "offsetWidth": 0,
        "offsetHeight": 0,
    })
}

impl Tables {
    pub fn get(&self, name: &str) -> Option<&Value> {
        match name {
            "traits" => self.traits.as_ref(),
            "layout" => self.layout.as_ref(),
            "style" => self.style.as_ref(),
            "media" => self.media.as_ref(),
            "shapes" => self.shapes.as_ref(),
            "timing" => self.timing.as_ref(),
            "webgl" => self.webgl.as_ref(),
            "graphics" => self.graphics.as_ref(),
            "viewport" => self.viewport.as_ref(),
            _ => None,
        }
    }

    pub fn flattened(&self) -> Self {
        let mut out = self.clone();

        if out.viewport.is_some() {
            out.viewport = Some(json!({
                "view": {
                    "innerWidth": 0,
                    "innerHeight": 0,
                    "visualViewportWidth": 0,
                    "visualViewportHeight": 0,
                },
                "documentElement": zero_box(),
                "body": zero_box(),
            }));
        }

        out
    }

    pub fn present(&self) -> Vec<&'static str> {
        [
            ("traits", self.traits.is_some()),
            ("layout", self.layout.is_some()),
            ("style", self.style.is_some()),
            ("media", self.media.is_some()),
            ("shapes", self.shapes.is_some()),
            ("timing", self.timing.is_some()),
            ("webgl", self.webgl.is_some()),
            ("graphics", self.graphics.is_some()),
            ("viewport", self.viewport.is_some()),
        ]
        .into_iter()
        .filter(|(_, held)| *held)
        .map(|(name, _)| name)
        .collect()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GraphProfile {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub captured_at: String,
    #[serde(default)]
    pub href: String,
    #[serde(default)]
    pub user_agent: String,
    pub snapshot: Value,
    #[serde(default)]
    pub tables: Tables,
}

impl GraphProfile {
    pub fn objects(&self) -> usize {
        self.snapshot
            .get("objects")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or_default()
    }

    pub fn read(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|error| Error::msg(format!("{}: {error}", path.display())))?;

        let mut profile: Self = serde_json::from_str(&text)
            .map_err(|error| Error::msg(format!("{}: {error}", path.display())))?;

        if profile.id.is_empty() {
            profile.id = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_default();
        }

        Ok(profile)
    }
}

#[derive(Debug, Clone, Default)]
pub struct GraphLibrary {
    paths: Vec<(String, std::path::PathBuf)>,
}

fn safe(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect();

    let trimmed = cleaned.trim_matches('-').to_lowercase();

    if trimmed.is_empty() {
        "profile".to_string()
    } else {
        trimmed
    }
}

impl GraphLibrary {
    pub fn load(dir: impl AsRef<std::path::Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let mut paths = Vec::new();

        if dir.is_dir() {
            let listing = std::fs::read_dir(dir)
                .map_err(|error| Error::msg(format!("{}: {error}", dir.display())))?;

            for entry in listing.flatten() {
                let path = entry.path();

                if path.extension().and_then(|kind| kind.to_str()) != Some("json") {
                    continue;
                }

                let id = path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().to_string())
                    .unwrap_or_default();

                paths.push((id, path));
            }
        }

        paths.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(Self { paths })
    }

    pub fn ids(&self) -> Vec<String> {
        self.paths.iter().map(|(id, _)| id.clone()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    pub fn dir(&self) -> Option<&std::path::Path> {
        self.paths.first().and_then(|(_, path)| path.parent())
    }

    pub fn store(
        dir: impl AsRef<std::path::Path>,
        profile: &GraphProfile,
        force: bool,
    ) -> Result<std::path::PathBuf> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir)
            .map_err(|error| Error::msg(format!("{}: {error}", dir.display())))?;

        let path = dir.join(format!("{}.json", safe(&profile.id)));

        if path.exists() && !force {
            return Err(Error::msg(format!(
                "{} is already there; pass --force to replace it",
                path.display()
            )));
        }

        let text = serde_json::to_string(profile)
            .map_err(|error| Error::msg(format!("the graph did not serialise: {error}")))?;

        std::fs::write(&path, text)
            .map_err(|error| Error::msg(format!("{}: {error}", path.display())))?;

        Ok(path)
    }

    pub fn resolve(&self, wanted: Option<&str>) -> Result<GraphProfile> {
        let chosen = match wanted {
            Some(id) => self
                .paths
                .iter()
                .find(|(known, _)| known == id)
                .ok_or_else(|| {
                    Error::msg(format!("no graph profile {id}; have {:?}", self.ids()))
                })?,
            None => self
                .paths
                .first()
                .ok_or_else(|| Error::msg("the graph profile library is empty"))?,
        };

        GraphProfile::read(&chosen.1)
    }
}

#[derive(Debug, Clone)]
pub struct GraphPage {
    pub url: String,
    pub referrer: String,
    pub entries: Vec<Value>,
    pub cookies: String,
    pub frames: usize,
    pub capture_cipher: bool,
}

impl Default for GraphPage {
    fn default() -> Self {
        Self {
            url: "about:blank".to_string(),
            referrer: String::new(),
            entries: Vec::new(),
            cookies: String::new(),
            frames: 4,
            capture_cipher: false,
        }
    }
}

pub struct Graph {
    realm: Realm,
    control: Control,
    misses: Misses,
    requests: Arc<Mutex<Vec<Request>>>,
    started: Instant,
}

#[derive(Clone)]
struct Bridges {
    snapshot: String,
    tables: Tables,
    clock: Instant,
    entries: Vec<Value>,
    misses: Misses,
    transport: Arc<dyn crate::browser::Transport>,
    recorded: Arc<Mutex<Vec<Request>>>,
}

fn install_hosts(
    realm: &mut Realm,
    frame: Option<usize>,
    shared: &Bridges,
    page: Value,
) -> Result<()> {
    let register = |realm: &mut Realm, name: &str, host: wre_live::realm::HostFn| match frame {
        None => realm.register_host(name, host),
        Some(index) => realm.register_host_in(index, name, host),
    };

    let snapshot = shared.snapshot.clone();
    register(
        realm,
        "__wreGraphSnapshot",
        Box::new(move |_args| Ok(json!(snapshot.clone()))),
    )?;

    let tables = shared.tables.clone();
    register(
        realm,
        "__wreGraphTable",
        Box::new(move |args| {
            let name = args.first().and_then(Value::as_str).unwrap_or_default();

            Ok(match tables.get(name) {
                Some(found) => json!(serde_json::to_string(found).unwrap_or_default()),
                None => Value::Null,
            })
        }),
    )?;

    let clock = shared.clock;
    register(
        realm,
        "__wreGraphNow",
        Box::new(move |_args| Ok(json!(clock.elapsed().as_secs_f64() * 1000.0))),
    )?;

    register(
        realm,
        "__wreGraphEntropy",
        Box::new(|args| {
            let wanted = args.first().and_then(Value::as_u64).unwrap_or_default() as usize;
            let mut bytes = vec![0u8; wanted.min(65_536)];
            rand::rng().fill(&mut bytes[..]);
            Ok(json!(STANDARD.encode(bytes)))
        }),
    )?;

    register(realm, "__wreGraphUuid", Box::new(|_args| Ok(json!(uuid()))))?;

    register(
        realm,
        "__wreGraphDigest",
        Box::new(|args| {
            let algorithm = args.first().and_then(Value::as_str).unwrap_or_default();
            let encoded = args.get(1).and_then(Value::as_str).unwrap_or_default();

            let Ok(bytes) = STANDARD.decode(encoded) else {
                return Ok(Value::Null);
            };

            Ok(match crate::host::digest_of(algorithm, &bytes) {
                Some(out) => json!(STANDARD.encode(out)),
                None => Value::Null,
            })
        }),
    )?;

    let entries = shared.entries.clone();
    register(
        realm,
        "__wreGraphEntries",
        Box::new(move |args| {
            let wanted = args.first().and_then(Value::as_str).unwrap_or_default();
            let matching: Vec<&Value> = entries
                .iter()
                .filter(|entry| entry.get("entryType").and_then(Value::as_str) == Some(wanted))
                .collect();

            Ok(json!(
                serde_json::to_string(&matching).unwrap_or_else(|_| "[]".to_string())
            ))
        }),
    )?;

    register(
        realm,
        "__wreGraphPage",
        Box::new(move |_args| Ok(page.clone())),
    )?;

    let support = shared
        .tables
        .media
        .clone()
        .unwrap_or_else(|| json!({ "video": {}, "audio": {} }));
    let watcher = shared.misses.clone();

    register(
        realm,
        "__wreGraphMedia",
        Box::new(move |args| {
            let wanted = args.first().and_then(Value::as_str).unwrap_or_default();

            for family in ["video", "audio"] {
                if let Some(answer) = support.get(family).and_then(|table| table.get(wanted)) {
                    return Ok(answer.clone());
                }
            }

            watcher.record(&format!("canPlayType({wanted})"));
            Ok(json!(""))
        }),
    )?;

    let transport = Arc::clone(&shared.transport);
    let recorded = Arc::clone(&shared.recorded);

    register(
        realm,
        "__wreGraphSend",
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

    let watcher = shared.misses.clone();
    register(
        realm,
        "__wreGraphMiss",
        Box::new(move |args| {
            if let Some(text) = args.first().and_then(Value::as_str) {
                watcher.record(text);
            }
            Ok(Value::Null)
        }),
    )?;

    Ok(())
}

const NATIVE: [(&str, &str); 2] = [
    ("Document.prototype", "createElement"),
    ("Permissions.prototype", "query"),
];

const SOURCES: &str = "globalThis.__SOURCES = new WeakMap();";

const SEED: &str =
    "globalThis.__SNAPSHOT = __wreGraphSnapshot(); delete globalThis.__wreGraphSnapshot;";
const EVALUATE: &str = "__ENV.evaluate = function (source) { return (0, eval)(source); };";

fn install_environment(realm: &mut Realm, frame: Option<usize>, capture: bool) -> Result<()> {
    let steps = [
        (SEED, ""),
        (BRIDGE, ""),
        (BOOTSTRAP, ""),
        (TABLES, ""),
        (
            if capture {
                "__ENV.captureCipher = true;"
            } else {
                ""
            },
            "",
        ),
        (DOM, ""),
        (EVALUATE, ""),
    ];

    for (source, name) in steps {
        if source.is_empty() {
            continue;
        }

        match frame {
            None => realm.eval_unit(source, name)?,
            Some(index) => realm.eval_unit_in(index, source, name)?,
        }
    }

    let ready = match frame {
        None => realm.eval_json("[typeof globalThis.__ENV, typeof globalThis.__PUMP].join()")?,
        Some(index) => realm.eval_json_in(
            index,
            "[typeof globalThis.__ENV, typeof globalThis.__PUMP].join()",
        )?,
    };

    if ready.as_str() != Some("object,object") {
        return Err(Error::msg(format!(
            "the graph environment did not finish installing: __ENV and __PUMP read {ready}"
        )));
    }

    Ok(())
}

pub fn open(
    profile: &GraphProfile,
    page: &GraphPage,
    hooks: Hooks,
    options: RealmOptions,
) -> Result<Graph> {
    let mut realm = Realm::new(RealmOptions {
        timers: false,
        codecs: false,
        ..options
    })?;
    let misses = Misses::default();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let started = Instant::now();

    let shared = Bridges {
        snapshot: serde_json::to_string(&profile.snapshot)
            .map_err(|error| Error::msg(format!("the graph did not serialise: {error}")))?,
        tables: profile.tables.clone(),
        clock: started,
        entries: page.entries.clone(),
        misses: misses.clone(),
        transport: Arc::clone(&hooks.transport),
        recorded: Arc::clone(&requests),
    };

    let framed = Bridges {
        tables: shared.tables.flattened(),
        ..shared.clone()
    };

    realm.eval_unit(SOURCES, "")?;

    for slot in 0..page.frames {
        let index = realm.open_frame()?;
        realm.share_into(index, "globalThis.__SOURCES", "__SOURCES")?;

        install_hosts(
            &mut realm,
            Some(index),
            &framed,
            json!({ "url": "about:blank", "referrer": page.url, "cookies": "" }),
        )?;

        realm.eval_unit_in(index, "globalThis.__FRAME = true;", "")?;
        install_environment(&mut realm, Some(index), false)?;

        realm.share_global(index, None, &format!("__wreFrameView{slot}"))?;
        realm.share_value(index, "globalThis.__ENV", &format!("__wreFrameEnv{slot}"))?;
        realm.share_value(index, "globalThis.__PUMP", &format!("__wreFramePump{slot}"))?;
        realm.eval_unit_in(
            index,
            "delete globalThis.__ENV; delete globalThis.__PUMP;",
            "",
        )?;
    }

    realm.set_global("__wreFrameCount", &json!(page.frames))?;

    install_hosts(
        &mut realm,
        None,
        &shared,
        json!({ "url": page.url, "referrer": page.referrer, "cookies": page.cookies }),
    )?;

    install_environment(&mut realm, None, page.capture_cipher)?;

    for (holder, key) in NATIVE {
        if realm.make_native(holder, key, Some(key)).is_err() {
            misses.record(&format!("{holder}.{key} could not be rebuilt as native"));
        }
    }

    let control = realm.attach(CONTROL, "")?;

    Ok(Graph {
        realm,
        control,
        misses,
        requests,
        started,
    })
}

const TABLES: &str = r#"
(function () {
  var read = function (name) {
    var raw = __wreGraphTable(name);
    if (raw === null || raw === undefined) return null;
    try { return JSON.parse(raw); } catch (error) { return null; }
  };

  var layout = read("layout");
  if (layout) __ENV.layout = layout;

  var timing = read("timing");
  if (timing) __ENV.useTiming(timing);

  var shapes = read("shapes");
  if (shapes) __ENV.shapes = shapes;

  var media = read("media");
  if (media) __ENV.media = media;

  var style = read("style");
  if (style) __ENV.styleShape = style;

  var graphics = read("graphics");
  if (graphics) __ENV.graphicsReplies = graphics;

  var webgl = read("webgl");
  if (webgl) __ENV.webglProfiles = webgl;

  var viewport = read("viewport");
  if (viewport) __ENV.viewport = viewport;

  var traits = read("traits");
  if (traits) __ENV.traits = traits;

  delete globalThis.__wreGraphTable;
})();
"#;

fn uuid() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill(&mut bytes[..]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;

    let hex = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

impl Graph {
    pub fn realm(&mut self) -> &mut Realm {
        &mut self.realm
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }

    pub fn run(&mut self, source: &str, name: &str, inline: bool) -> Result<Option<Value>> {
        self.realm
            .invoke(&self.control, "begin", &[json!(name), json!(inline)])?;

        let outcome = self.realm.eval_unit(source, name);

        self.realm.invoke(&self.control, "end", &[])?;

        Ok(match outcome {
            Ok(()) => None,
            Err(error) => Some(json!({ "message": error.to_string() })),
        })
    }

    pub fn make_native(&mut self, holder: &str, key: &str, name: Option<&str>) -> Result<()> {
        self.realm.make_native(holder, key, name)
    }

    pub fn eval(&mut self, source: &str, name: &str) -> Result<()> {
        self.realm.eval_unit(source, name)
    }

    pub fn read(&mut self, expression: &str) -> Result<Value> {
        self.realm
            .invoke(&self.control, "read", &[json!(expression)])
    }

    pub fn step(&mut self) -> Result<usize> {
        let ran = self.realm.invoke(&self.control, "step", &[])?;
        Ok(ran.as_u64().unwrap_or_default() as usize)
    }

    pub fn pending(&mut self) -> Result<usize> {
        let waiting = self.realm.invoke(&self.control, "pending", &[])?;
        Ok(waiting.as_u64().unwrap_or_default() as usize)
    }

    pub fn cookies(&mut self) -> Result<String> {
        let value = self.realm.invoke(&self.control, "cookies", &[])?;
        Ok(value.as_str().unwrap_or_default().to_string())
    }

    pub fn calls(&mut self) -> Result<Value> {
        self.realm.invoke(&self.control, "calls", &[])
    }

    pub fn trail(&mut self) -> Result<Value> {
        self.realm.invoke(&self.control, "trail", &[])
    }

    pub fn log(&mut self) -> Result<Value> {
        self.realm.invoke(&self.control, "log", &[])
    }

    pub fn misses(&mut self) -> Vec<String> {
        let mut out = self.misses.all();
        out.extend(self.counted(false));
        out
    }

    pub fn guards(&mut self) -> Vec<String> {
        self.counted(true)
    }

    fn counted(&mut self, illegal: bool) -> Vec<String> {
        let Ok(Value::Object(fields)) = self.realm.invoke(&self.control, "misses", &[]) else {
            return Vec::new();
        };

        fields
            .into_iter()
            .filter(|(name, _)| name.starts_with("illegal:") == illegal)
            .map(|(name, count)| format!("{} x{count}", name.trim_start_matches("illegal:")))
            .collect()
    }

    pub fn requests(&self) -> Vec<Request> {
        let list = self
            .requests
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        list.clone()
    }
}
