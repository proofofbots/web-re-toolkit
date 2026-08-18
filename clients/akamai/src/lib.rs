pub mod cookies;
pub mod discover;
pub mod pixel;
pub mod pow;
pub mod sensor;
pub mod session;

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use wre_client::client::{Client, Registration};
use wre_client::context::{Call, Ctx, FetchRequest, HttpOptions, Jar};
use wre_client::error::{ClientError, ClientResult};
use wre_client::shape::{Shape, field};
use wre_client::spec::{Capabilities, ClientDescriptor, Concurrency, OpSpec};
use wre_sandbox::machine;
use wre_sandbox::profile::Profile;
use wre_sandbox::library::{BUILTIN_ID, Library, Record};

use session::{Session, Settings};

pub const ID: &str = "akamai";

const NOTES: &str = r#"## How a run works

`solve` is the whole flow. It loads the page, mounts the page's own sensor script in the sandbox, plays a pointer and key stream into it, reads back the telemetry the script builds, and posts that payload to the collection endpoint the script names. What comes back is the payload it sent, the endpoint it used, and the cookie jar afterwards, `_abck` included.

Every other op runs against the session `solve` opened, so call it first. `request` sends a request of yours through that same jar, transport fingerprint and user agent, which is the point of warming a session at all. `payload` builds another sensor payload without reloading the page, `post` sends one, `cookies` reads the jar, and `page` returns the last HTML with its form fields already parsed.

`discover` is the cheap look: it fetches the page and reports which Akamai pieces it carries, without mounting anything.

## Sessions and cookies

A session is a client. One client holds one jar, one sandbox realm and one transport fingerprint, so keep it open for the whole flow instead of opening one per call. `reset` drops the realm and starts a new one, and `reset` with `cookies` set also empties the jar.

A valid `_abck` is not a guarantee. The edge scores what the payload contains, and a site can also demand the `sec_cpt` proof of work, which `pow` answers, or serve the pixel challenge, which runs automatically unless you turn `pixel` off.

## Profiles

The sensor is fingerprinted against a browser, so the sandbox mounts a captured device profile. Without a workspace on disk the client falls back to the profile built into the binary, which carries a whole browser surface and is served. Capture your own with `wre sandbox capture`, list them with `wre sandbox list`, then name one in `profile`, or set `random_profile` to pick from the library per session. A captured profile carries the canvas the machine renders and the boxes its pages lay out, which the built in one cannot. The transport fingerprint follows the profile's user agent unless `fingerprint` overrides it.

`farble` perturbs what the profile renders, per session, the way a privacy browser does: the canvas comes back a pixel different, the audio render sums a fraction off and text measures on a scale of its own. The device the profile describes does not change. `run.rendered` names what a session rendered, so two of them can be told apart.

## Time

A session takes about two seconds, most of it round trips to the origin. The waits are ceilings on conditions rather than durations: `load_posts_ms` ends when the script's own load posts have been answered, `wait_ms` when the edge has validated the jar and the page has nothing left in flight. `warp` is how much faster the page's clock runs than the wall while those run, which is what keeps a script that reposts every few seconds from costing every one of them.

Warming a session is the expensive part, and `request` on a warm one is a single round trip, so a hundred requests want one session rather than a hundred."#;

pub fn registration() -> Registration {
    Registration { id: ID, describe, build }
}

pub fn describe() -> ClientDescriptor {
    ClientDescriptor::new(ID, env!("CARGO_PKG_VERSION"))
        .summary("Runs an Akamai Bot Manager sensor headlessly and carries the session it produces")
        .primary("solve")
        .notes(NOTES)
        .capabilities(Capabilities {
            needs_v8: true,
            needs_chrome: false,
            needs_network: true,
            stateful: true,
            concurrency: Concurrency::PerSession,
            warmup_ms: 0,
        })
        .config(config_shape())
        .op(
            OpSpec::new(
                "info",
                Shape::object("InfoInput", []),
                Shape::object(
                    "Info",
                    [
                        field("target", Shape::Str),
                        field("version", Shape::Str),
                        field("profile", Shape::Str).summary("Sandbox profile this session mounted"),
                        field("profiles", Shape::list(Shape::Str))
                            .summary("Every profile id the workspace library holds"),
                        field("user_agent", Shape::Str),
                        field("fingerprint", Shape::Str)
                            .summary("Transport fingerprint, or where it is being derived from"),
                        field("open", Shape::Bool).summary("Whether a page session is open"),
                    ],
                ),
            )
            .summary("What this build is carrying"),
        )
        .op(
            OpSpec::new(
                "discover",
                Shape::object(
                    "DiscoverInput",
                    [field("url", Shape::optional(Shape::Str))
                        .summary("Page to fetch, defaults to page_url from the config")],
                ),
                Shape::object(
                    "Discovered",
                    [
                        field("url", Shape::Str).summary("Url after redirects"),
                        field("status", Shape::Int),
                        field("protected", Shape::Bool)
                            .summary("Whether the page carries an Akamai sensor"),
                        field("surface", Shape::Json)
                            .summary("Script urls, the collection endpoint and the challenge page when there is one"),
                        field("cookies", Shape::Json)
                            .summary("Jar after the fetch, with _abck and sec_cpt broken out"),
                        field("set_cookie", Shape::list(Shape::Str))
                            .summary("Cookie names the navigation response set"),
                    ],
                ),
            )
            .summary("Report the Akamai surface of a page without running anything")
            .deadline_ms(45_000),
        )
        .op(
            OpSpec::new(
                "solve",
                Shape::object(
                    "SolveInput",
                    [
                        field("url", Shape::optional(Shape::Str))
                            .summary("Page to load, defaults to page_url from the config"),
                        field("rounds", Shape::optional(Shape::Int))
                            .summary("How many payloads to post before returning"),
                        field("wait_ms", Shape::optional(Shape::Int))
                            .summary("Overrides how long the sensor is left running after load"),
                        field("post", Shape::optional(Shape::Bool))
                            .summary("Post the payload, on by default"),
                    ],
                ),
                Shape::object(
                    "Solved",
                    [
                        field("url", Shape::Str).summary("Page the session loaded"),
                        field("telemetry", Shape::optional(Shape::Str))
                            .summary("Value for the akamai-bm-telemetry header"),
                        field("payload", Shape::optional(Shape::Str))
                            .summary("Sensor payload as posted, sensor_data and all"),
                        field("endpoint", Shape::optional(Shape::Str))
                            .summary("Collection endpoint the script named"),
                        field("posts", Shape::Json)
                            .summary("One entry per post, with its status and response"),
                        field("cookies", Shape::Json)
                            .summary("Jar after the run, with _abck and sec_cpt broken out"),
                        field("challenge", Shape::Json)
                            .summary("Proof of work items and challenge page, when the edge asked for either"),
                        field("run", Shape::Json)
                            .summary("What the sandbox did: scripts, timers, requests and misses"),
                    ],
                ),
            )
            .summary("Load the page, run its sensor and post what it builds")
            .deadline_ms(180_000),
        )
        .op(
            OpSpec::new(
                "payload",
                Shape::object(
                    "PayloadInput",
                    [field("nudge_ms", Shape::optional(Shape::Int))
                        .summary("Advance the page clock this far first, so the payload is not a repeat")],
                ),
                Shape::object(
                    "Payload",
                    [
                        field("telemetry", Shape::optional(Shape::Str))
                            .summary("Value for the akamai-bm-telemetry header"),
                        field("payload", Shape::optional(Shape::Str))
                            .summary("Sensor payload, ready to post"),
                        field("endpoint", Shape::optional(Shape::Str))
                            .summary("Collection endpoint the script named"),
                    ],
                ),
            )
            .summary("Build a fresh payload from the session that is already open")
            .deadline_ms(60_000),
        )
        .op(
            OpSpec::new(
                "post",
                Shape::object(
                    "PostInput",
                    [
                        field("payload", Shape::optional(Shape::Str))
                            .summary("Payload to send, defaults to a fresh one from the session"),
                        field("endpoint", Shape::optional(Shape::Str))
                            .summary("Where to send it, defaults to the endpoint the script named"),
                        field("rounds", Shape::optional(Shape::Int))
                            .summary("Posts to make, one by default"),
                    ],
                ),
                Shape::object(
                    "Posted",
                    [
                        field("posts", Shape::Json)
                            .summary("One entry per post, with its status and response"),
                        field("cookies", Shape::Json).summary("Jar after the posts"),
                    ],
                ),
            )
            .summary("Post a payload to the collection endpoint")
            .deadline_ms(90_000),
        )
        .op(
            OpSpec::new(
                "request",
                Shape::object(
                    "RequestInput",
                    [
                        field("url", Shape::Str),
                        field("method", Shape::optional(Shape::Str)).summary("GET by default"),
                        field("headers", Shape::optional(Shape::map(Shape::Str)))
                            .summary("Added on top of the ones the session already sends"),
                        field("body", Shape::optional(Shape::Str))
                            .summary("Raw body, which wins over form and json"),
                        field("telemetry", Shape::optional(Shape::Bool))
                            .summary("Attach a fresh akamai-bm-telemetry header"),
                        field("form", Shape::optional(Shape::map(Shape::Str)))
                            .summary("Form encode these fields as the body"),
                        field("json", Shape::optional(Shape::Json))
                            .summary("Send this as a json body"),
                        field("kind", Shape::optional(Shape::Str)).summary(
                            "form sends it the way the page submits a form, otherwise it goes as an XHR",
                        ),
                    ],
                ),
                Shape::object(
                    "Answered",
                    [
                        field("status", Shape::Int),
                        field("url", Shape::Str).summary("Url after redirects"),
                        field("headers", Shape::Json)
                            .summary("Response headers as name and value pairs, without set-cookie"),
                        field("body", Shape::Str),
                        field("cookies", Shape::Json).summary("Jar after the request"),
                        field("refused", Shape::Bool)
                            .summary("True on 403, 429, an access denied body or a challenge redirect"),
                    ],
                ),
            )
            .summary("Send a request carrying the session this client warmed")
            .deadline_ms(90_000),
        )
        .op(
            OpSpec::new(
                "relay",
                Shape::object(
                    "RelayInput",
                    [
                        field("url", Shape::Str),
                        field("method", Shape::optional(Shape::Str)).summary("GET by default"),
                        field("headers", Shape::optional(Shape::Json)).summary(
                            "Name and value pairs sent verbatim, in this order, with nothing added",
                        ),
                        field("body_b64", Shape::optional(Shape::Str))
                            .summary("Request body, base64"),
                    ],
                ),
                Shape::object(
                    "Relayed",
                    [
                        field("status", Shape::Int),
                        field("url", Shape::Str).summary("Url after redirects"),
                        field("headers", Shape::Json)
                            .summary("Every response header, set-cookie included"),
                        field("body_b64", Shape::Str).summary("Response body, base64"),
                        field("cookies", Shape::Json).summary("Jar after the request"),
                    ],
                ),
            )
            .summary("Relay a request byte for byte through this session's transport and jar")
            .deadline_ms(90_000),
        )
        .op(
            OpSpec::new(
                "page",
                Shape::object("PageInput", []),
                Shape::object(
                    "PageState",
                    [
                        field("url", Shape::Str),
                        field("html", Shape::Str),
                        field("fields", Shape::map(Shape::Str))
                            .summary("Every input the page declares, by name, with its value"),
                        field("bytes", Shape::Int).summary("Size of the html"),
                    ],
                ),
            )
            .summary("The page this session last loaded, with the form fields it declares"),
        )
        .op(
            OpSpec::new(
                "cookies",
                Shape::object(
                    "CookiesInput",
                    [field("set", Shape::optional(Shape::Str))
                        .summary("A Cookie header to seed the jar with before reading it back")],
                ),
                Shape::object(
                    "Cookies",
                    [
                        field("header", Shape::Str)
                            .summary("The jar as a Cookie header, ready to send elsewhere"),
                        field("summary", Shape::Json)
                            .summary("_abck and sec_cpt read out, with the work items they carry"),
                        field("names", Shape::list(Shape::Str)),
                    ],
                ),
            )
            .summary("What the jar is holding for this session"),
        )
        .op(
            OpSpec::new(
                "pow",
                Shape::object(
                    "PowInput",
                    [
                        field("abck", Shape::optional(Shape::Str))
                            .summary("Cookie to read the work items from, defaults to the session's"),
                        field("challenge", Shape::optional(Shape::Str))
                            .summary("A single challenge string, id-token-salt-difficulty-delay-slice"),
                        field("start_ts", Shape::optional(Shape::Int))
                            .summary("Page start timestamp the hash is built from, taken from the open session otherwise"),
                        field("rounds", Shape::optional(Shape::Int))
                            .summary("Nonces to find, one per slice"),
                    ],
                ),
                Shape::object(
                    "PowAnswer",
                    [
                        field("challenge", Shape::Json)
                            .summary("Work item as parsed: id, token, salt, difficulty, delay and slice"),
                        field("prefix", Shape::Str).summary("String each nonce is appended to"),
                        field("nonces", Shape::list(Shape::Str)),
                        field("attempts", Shape::list(Shape::Int))
                            .summary("Hashes tried per round"),
                        field("elapsed_ms", Shape::list(Shape::Int)),
                        field("answer", Shape::Str)
                            .summary("Formatted answer, what the sec_cpt endpoint expects"),
                    ],
                ),
            )
            .summary("Solve the proof of work the _abck cookie asks for")
            .deadline_ms(120_000),
        )
        .op(
            OpSpec::new(
                "pixel",
                Shape::object("PixelInput", []),
                Shape::object(
                    "Pixel",
                    [field("pixel", Shape::Json)
                        .summary("What the pixel client sent and what came back, or why it was skipped")],
                ),
            )
            .summary("Run the pixel challenge client in the session that is open")
            .deadline_ms(60_000),
        )
        .op(
            OpSpec::new(
                "script",
                Shape::object("ScriptInput", []),
                Shape::object(
                    "Script",
                    [
                        field("url", Shape::Str).summary("Where the sensor script came from"),
                        field("bytes", Shape::Int),
                        field("source", Shape::Str)
                            .summary("The exact body the sandbox ran, which the edge varies per fetch"),
                    ],
                ),
            )
            .summary("Hand back the sensor script this session actually ran"),
        )
        .op(
            OpSpec::new(
                "eval",
                Shape::object(
                    "EvalInput",
                    [
                        field("expression", Shape::Str)
                            .summary("JavaScript evaluated in the mounted realm, result serialised as JSON"),
                        field("nudge_ms", Shape::optional(Shape::Int))
                            .summary("Advance the page clock this far before evaluating"),
                    ],
                ),
                Shape::object(
                    "Evaluated",
                    [
                        field("value", Shape::Json).summary("What the expression returned"),
                        field("error", Shape::optional(Shape::Str))
                            .summary("Set instead of value when the expression threw"),
                    ],
                ),
            )
            .summary("Evaluate an expression inside the sandbox the sensor is running in")
            .deadline_ms(60_000),
        )
        .op(
            OpSpec::new(
                "reset",
                Shape::object(
                    "ResetInput",
                    [field("cookies", Shape::optional(Shape::Bool))
                        .summary("Empty the jar too, off by default")],
                ),
                Shape::object("Reset", [field("open", Shape::Bool)]),
            )
            .summary("Drop the sandbox, and the cookies when asked"),
        )
}

fn config_shape() -> Shape {
    Shape::object(
        "AkamaiConfig",
        [
            field("page_url", Shape::optional(Shape::Str))
                .summary("Page the session warms against when an op does not name one"),
            field("profile", Shape::optional(Shape::Str))
                .summary("Sandbox profile id, from `wre sandbox list`"),
            field("random_profile", Shape::Bool)
                .summary("Pick a captured profile at random")
                .with_default(json!(false)),
            field("random_machine", Shape::Bool)
                .summary("Put every session on a different Mac: the chip, its core count and its \
                          memory, picked together so they agree. What the machine renders is left \
                          as captured, because two Macs running the same macOS draw the same canvas")
                .with_default(json!(true)),
            field("proxy", Shape::optional(Shape::Str))
                .summary("Proxy url the session and the sandbox both go through, http or socks5"),
            field("fingerprint", Shape::optional(Shape::Str))
                .summary("Transport fingerprint as profile[:platform], defaults to the sandbox profile's user agent"),
            field("user_agent", Shape::optional(Shape::Str))
                .summary("Overrides the user agent the sandbox profile carries"),
            field("wait_ms", Shape::Int)
                .summary("Ceiling on the wait after the input stream. It ends as soon as the edge \
                          has validated the jar and the page has nothing left in flight, so a \
                          session that is believed early pays none of it")
                .with_default(json!(6_000)),
            field("init_cost_ms", Shape::Int)
                .summary("Clock charge applied when the sensor writes bmak.startTs")
                .with_default(json!(25)),
            field("friction_ms", Shape::Float)
                .summary("Virtual cost of one DOM operation")
                .with_default(json!(0.12)),
            field("script_rate", Shape::Float)
                .summary("How much of a script's own run time the page's clock counts. The \
                          interpreter is slower than V8, so counting all of it puts the sensor's \
                          own timings an order out")
                .with_default(json!(0.07)),
            field("behaviour", Shape::Bool)
                .summary("Play a pointer, click and key stream into the page")
                .with_default(json!(true)),
            field("pixel", Shape::Bool)
                .summary("Run the pixel challenge client when the page serves one")
                .with_default(json!(true)),
            field("keep_payloads", Shape::Bool)
                .summary("Carry the sensor payload of every post in the answer, for diffing")
                .with_default(json!(false)),
            field("prelude", Shape::optional(Shape::Str))
                .summary("Source run in the realm before the sensor script, for recorders"),
            field("load_posts_ms", Shape::Float)
                .summary("How long to let the script's own load-time posts land before input \
                          starts")
                .with_default(json!(4_000.0)),
            field("warp", Shape::Float)
                .summary("How much faster the page's clock runs than the wall while the session \
                          waits. The scripts schedule their posts seconds apart and the edge reads \
                          the payload, not the wall, so 1 spends every one of those seconds and 12 \
                          spends a twelfth of them")
                .with_default(json!(16.0)),
            field("typed", Shape::Str)
                .summary("Text typed into the page's first text field. Empty picks a short word \
                          from the session seed")
                .with_default(json!("")),
            field("live_xhr", Shape::Bool)
                .summary("Let the sensor's own requests leave the sandbox, which is what the \
                          script does in a browser. Turning it off has the host post what the \
                          sandbox builds instead, one payload per round, and the edge reads that \
                          differently")
                .with_default(json!(true)),
            field("wire_pace", Shape::Bool)
                .summary("Hold each live request until the wall clock reaches the time the \
                          sandbox believes it is. The edge does not read a payload against when it \
                          arrives, so this is off by default and a session finishes in the time \
                          the work takes rather than the time the page would have spent")
                .with_default(json!(false)),
            field("rounds", Shape::Int)
                .summary("Payloads posted per solve")
                .with_default(json!(2)),
            field("timeout_ms", Shape::Int)
                .summary("Cap on one http request the session makes")
                .with_default(json!(90_000)),
            field("seed", Shape::optional(Shape::Int))
                .summary("Seed the behaviour stream and the random source"),
            field("workers", Shape::Int)
                .summary("Threads the proof of work search uses, 0 picks four")
                .with_default(json!(0)),
            field("max_attempts", Shape::Int)
                .summary("Highest nonce the proof of work search tries per round")
                .with_default(json!(5_000_000)),
        ],
    )
}

#[derive(Debug, Clone, Deserialize)]
struct Config {
    #[serde(default)]
    page_url: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    random_profile: bool,
    #[serde(default = "yes")]
    random_machine: bool,
    #[serde(default)]
    proxy: Option<String>,
    #[serde(default)]
    fingerprint: Option<String>,
    #[serde(default)]
    user_agent: Option<String>,
    #[serde(default = "default_wait")]
    wait_ms: u64,
    #[serde(default = "default_init_cost")]
    init_cost_ms: f64,
    #[serde(default = "default_friction")]
    friction_ms: f64,
    #[serde(default = "default_script_rate")]
    script_rate: f64,
    #[serde(default = "yes")]
    behaviour: bool,
    #[serde(default = "yes")]
    pixel: bool,
    #[serde(default = "yes")]
    live_xhr: bool,
    #[serde(default)]
    wire_pace: bool,
    #[serde(default)]
    prelude: Option<String>,
    #[serde(default = "default_load_posts")]
    load_posts_ms: f64,
    #[serde(default = "default_warp")]
    warp: f64,
    #[serde(default = "default_typed")]
    typed: String,
    #[serde(default)]
    keep_payloads: bool,
    #[serde(default = "default_rounds")]
    rounds: usize,
    #[serde(default = "default_timeout")]
    timeout_ms: u64,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default)]
    workers: usize,
    #[serde(default = "default_attempts")]
    max_attempts: u64,
}

fn default_wait() -> u64 {
    6_000
}

fn default_init_cost() -> f64 {
    25.0
}

fn default_friction() -> f64 {
    0.12
}

fn default_script_rate() -> f64 {
    0.07
}

fn yes() -> bool {
    true
}

fn default_load_posts() -> f64 {
    4_000.0
}

fn default_warp() -> f64 {
    16.0
}

fn default_typed() -> String {
    String::new()
}

fn default_rounds() -> usize {
    2
}

fn default_timeout() -> u64 {
    90_000
}

fn default_attempts() -> u64 {
    5_000_000
}

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_nanos() as u64)
        .unwrap_or_default()
}

fn build(ctx: Ctx, config: Value) -> ClientResult<Box<dyn Client>> {
    let config: Config = serde_json::from_value(config)
        .map_err(|error| ClientError::bad_input(format!("config rejected: {error}")))?;

    let mut ctx = ctx;
    if let Some(seed) = config.seed {
        ctx = ctx.with_seed(seed);
    }

    let record = resolve_profile(&ctx, &config)?;

    let user_agent = config.user_agent.clone().unwrap_or_else(|| {
        record
            .profile
            .property("Navigator", "userAgent")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string()
    });

    if user_agent.is_empty() {
        return Err(ClientError::bad_input(
            "the sandbox profile carries no user agent, set user_agent in the config",
        ));
    }

    ctx.fact("profile", json!(record.id));
    ctx.fact("user_agent", json!(user_agent));
    ctx.fact("page_url", json!(config.page_url));

    Ok(Box::new(Akamai {
        ctx,
        config,
        record,
        user_agent,
        jar: Jar::new(),
        session: None,
        last_run: Value::Null,
        realms: 0,
    }))
}

fn resolve_profile(ctx: &Ctx, config: &Config) -> ClientResult<Record> {
    let Some(workspace) = ctx.workspace() else {
        return Ok(Record::builtin());
    };

    let library = Library::load(workspace.join("profiles"))
        .map_err(|error| ClientError::resource(format!("the profile library failed: {error}")))?;

    library
        .resolve(config.profile.as_deref(), config.random_profile)
        .map_err(|error| ClientError::bad_input(error.to_string()))
}

struct Akamai {
    ctx: Ctx,
    config: Config,
    record: Record,
    user_agent: String,
    jar: Jar,
    session: Option<Session>,
    last_run: Value,
    realms: u64,
}

impl Akamai {
    fn settings(&self) -> Settings {
        Settings {
            wait_ms: self.config.wait_ms as f64,
            init_cost_ms: self.config.init_cost_ms,
            friction_ms: self.config.friction_ms,
            script_rate: self.config.script_rate,
            behaviour: self.config.behaviour,
            pixel: self.config.pixel,
            live_xhr: self.config.live_xhr,
            wire_pace: self.config.wire_pace,
            keep_payloads: self.config.keep_payloads,
            timeout_ms: self.config.timeout_ms,
            seed: self.config.seed.unwrap_or(0),
            prelude: self.config.prelude.clone(),
            load_posts_ms: self.config.load_posts_ms,
            warp: self.config.warp,
            typed: self.config.typed.clone(),
        }
    }

    fn varied(&mut self) -> Profile {
        let mut profile = self.record.profile.clone();

        if !self.config.random_machine {
            return profile;
        }

        self.realms += 1;

        let seed = match self.config.seed {
            Some(seed) => seed.wrapping_mul(0x9e37_79b9).wrapping_add(self.realms),
            None => now_ns() ^ self.realms.wrapping_mul(0x2545_f491_4f6c_dd1d),
        };

        machine::vary(&mut profile, seed);
        profile
    }

    fn fresh(&mut self) -> ClientResult<Session> {
        let profile = self.varied();
        let mut options = HttpOptions::with_proxy(self.config.proxy.as_deref());
        options.fingerprint = self.config.fingerprint.clone();
        options.user_agent = Some(self.user_agent.clone());
        options.claim = session::claim_of(&profile, &self.user_agent);
        options.timeout_secs = Some(self.config.timeout_ms.div_ceil(1000).max(1));
        options.jar = Some(self.jar.clone());

        let http = Arc::new(self.ctx.http_with(options)?);

        Ok(Session::new(
            http,
            self.jar.clone(),
            profile,
            self.record.id.clone(),
            self.settings(),
            self.user_agent.clone(),
        ))
    }

    fn session(&mut self) -> ClientResult<&mut Session> {
        if self.session.is_none() {
            let session = self.fresh()?;
            self.session = Some(session);
        }

        Ok(self.session.as_mut().expect("session"))
    }

    fn open_session(&mut self) -> ClientResult<&mut Session> {
        let session = self.session()?;

        if !session.is_open() {
            return Err(ClientError::bad_input(
                "no session is open, call solve first",
            ));
        }

        Ok(session)
    }

    fn page(&self, params: &Value) -> ClientResult<String> {
        params
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| self.config.page_url.clone())
            .ok_or_else(|| ClientError::bad_input("no url, pass one or set page_url in the config"))
    }

}

fn form_fields(html: &str) -> Map<String, Value> {
    let mut out = Map::new();
    let pattern = regex::Regex::new(r#"(?is)<input\b([^>]*)>"#).expect("input pattern");
    let attribute = regex::Regex::new(r#"(?is)([a-zA-Z_:][-a-zA-Z0-9_:.]*)\s*=\s*"([^"]*)""#)
        .expect("attribute pattern");

    for tag in pattern.captures_iter(html) {
        let text = tag.get(1).map_or("", |part| part.as_str());
        let mut name = None;
        let mut value = String::new();

        for found in attribute.captures_iter(text) {
            match found.get(1).map(|part| part.as_str().to_lowercase()).unwrap_or_default().as_str() {
                "name" => name = found.get(2).map(|part| part.as_str().to_string()),
                "value" => value = found.get(2).map_or("", |part| part.as_str()).to_string(),
                _ => {}
            }
        }

        if let Some(name) = name {
            out.insert(name, Value::String(value));
        }
    }

    out
}

fn form_encode(fields: &Map<String, Value>) -> String {
    fields
        .iter()
        .map(|(name, value)| {
            let text = match value {
                Value::String(found) => found.clone(),
                other => other.to_string(),
            };
            format!("{}={}", escape(name), escape(&text))
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }

    out
}

fn refused(status: u16, body: &str) -> bool {
    if status == 403 || status == 429 {
        return true;
    }

    let lowered = body.to_lowercase();
    lowered.contains("access denied") || lowered.contains("_sec/cp_challenge")
}

impl Client for Akamai {
    fn call(&mut self, op: &str, params: Value, call: &Call) -> ClientResult<Value> {
        call.check()?;

        match op {
            "info" => {
                let profiles = match self.ctx.workspace() {
                    Some(workspace) => Library::load(workspace.join("profiles"))
                        .map(|library| library.ids())
                        .unwrap_or_default(),
                    None => Vec::new(),
                };

                let open = self.session.as_ref().map(Session::is_open).unwrap_or(false);

                Ok(json!({
                    "target": ID,
                    "version": env!("CARGO_PKG_VERSION"),
                    "profile": self.record.id,
                    "profiles": profiles,
                    "user_agent": self.user_agent,
                    "fingerprint": self.config.fingerprint.clone().unwrap_or_else(|| "from user agent".to_string()),
                    "open": open,
                }))
            }

            "discover" => {
                let url = self.page(&params)?;
                let session = self.session()?;
                let response = session.navigate(&url)?;

                let set_cookie: Vec<String> = response
                    .headers
                    .iter()
                    .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
                    .map(|(_, value)| value.split('=').next().unwrap_or_default().to_string())
                    .collect();

                Ok(json!({
                    "url": session.page_url(),
                    "status": response.status,
                    "protected": session.surface().is_protected(),
                    "surface": session.surface(),
                    "cookies": session.cookies(),
                    "set_cookie": set_cookie,
                }))
            }

            "solve" => {
                let url = self.page(&params)?;
                let rounds = params
                    .get("rounds")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize)
                    .unwrap_or(self.config.rounds);
                let post = params
                    .get("post")
                    .and_then(Value::as_bool)
                    .unwrap_or(!self.config.live_xhr);

                if let Some(wait) = params.get("wait_ms").and_then(Value::as_u64) {
                    self.config.wait_ms = wait;
                }

                let mut session = self.fresh()?;
                let run = session.open(&url)?;
                self.last_run = run.clone();
                call.check()?;

                let telemetry = session.telemetry()?;
                let payload = session.payload()?;
                let endpoint = session.endpoint();

                let Some(first) = payload.clone() else {
                    self.session = Some(session);
                    return Err(ClientError::internal(
                        "the sensor produced no payload",
                    )
                    .with_detail(run));
                };

                let mut posts: Vec<Value> = session
                    .posts()
                    .iter()
                    .map(|sent| serde_json::to_value(sent).unwrap_or(Value::Null))
                    .collect();

                if post {
                    for round in 0..rounds {
                        call.check()?;

                        let payload = if round == 0 {
                            first.clone()
                        } else {
                            session.nudge(1500.0)?;
                            session.payload()?.unwrap_or_else(|| first.clone())
                        };

                        let sent = session.post_payload(&payload, endpoint.as_deref())?;
                        posts.push(serde_json::to_value(&sent).unwrap_or(Value::Null));
                        call.progress(round as u64 + 1, rounds as u64, "posted");
                    }
                }

                let cookies = session.cookies();
                let challenge = json!({
                    "sec_cpt": cookies.sec_cpt,
                    "pow": cookies
                        .abck
                        .as_ref()
                        .map(|abck| abck.challenges.clone())
                        .unwrap_or_default(),
                    "page": session.surface().challenge_page,
                });

                let out = json!({
                    "url": session.page_url(),
                    "telemetry": telemetry,
                    "payload": payload,
                    "endpoint": endpoint,
                    "posts": posts,
                    "cookies": cookies,
                    "challenge": challenge,
                    "run": run,
                });

                self.session = Some(session);
                Ok(out)
            }

            "payload" => {
                let nudge = params.get("nudge_ms").and_then(Value::as_f64);
                let session = self.open_session()?;

                if let Some(ms) = nudge {
                    session.nudge(ms)?;
                }

                Ok(json!({
                    "telemetry": session.telemetry()?,
                    "payload": session.payload()?,
                    "endpoint": session.endpoint(),
                }))
            }

            "post" => {
                let rounds = params
                    .get("rounds")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize)
                    .unwrap_or(1);
                let given = params.get("payload").and_then(Value::as_str).map(str::to_string);
                let endpoint = params.get("endpoint").and_then(Value::as_str).map(str::to_string);

                let session = self.open_session()?;
                let mut posts = Vec::new();

                for round in 0..rounds.max(1) {
                    call.check()?;

                    let payload = match &given {
                        Some(found) => found.clone(),
                        None => {
                            if round > 0 {
                                session.nudge(1500.0)?;
                            }
                            session.payload()?.ok_or_else(|| {
                                ClientError::internal("the sensor produced no payload")
                            })?
                        }
                    };

                    let sent = session.post_payload(&payload, endpoint.as_deref())?;
                    posts.push(serde_json::to_value(&sent).unwrap_or(Value::Null));
                }

                Ok(json!({ "posts": posts, "cookies": session.cookies() }))
            }

            "request" => {
                let url = params
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ClientError::bad_input("request needs a url"))?
                    .to_string();

                let method = params
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or("GET")
                    .to_uppercase();

                let telemetry = params.get("telemetry").and_then(Value::as_bool).unwrap_or(false);

                let body = match (
                    params.get("body").and_then(Value::as_str),
                    params.get("form").and_then(Value::as_object),
                    params.get("json"),
                ) {
                    (Some(text), _, _) => Some(text.as_bytes().to_vec()),
                    (None, Some(fields), _) => Some(form_encode(fields).into_bytes()),
                    (None, None, Some(value)) if !value.is_null() => {
                        Some(value.to_string().into_bytes())
                    }
                    _ => None,
                };

                let user_agent = self.user_agent.clone();
                let languages = session::accept_language(&self.record.profile);
                let session = self.session()?;

                let header = if telemetry { session.telemetry()? } else { None };

                let submitted = params.get("kind").and_then(Value::as_str) == Some("form");

                let order = if submitted { &session::FORM_ORDER[..] } else { &session::XHR_ORDER[..] };

                let mut request = FetchRequest {
                    url: url.clone(),
                    method,
                    headers: Vec::new(),
                    body,
                    fingerprint: None,
                    order: order.iter().map(|name| name.to_string()).collect(),
                };

                request = request
                    .header("accept-language", languages)
                    .header("user-agent", user_agent);

                if submitted {
                    for (name, value) in session::FORM {
                        request = request.header(name, value);
                    }
                } else {
                    request = request.header("accept", "*/*");
                }

                if !session.page_url().is_empty() {
                    request = request.header("referer", session.page_url().to_string());

                    if request.method != "GET" {
                        if let Ok(parsed) = url::Url::parse(session.page_url()) {
                            if let Some(host) = parsed.host_str() {
                                let origin = format!("{}://{}", parsed.scheme(), host);
                                request = request.header("origin", origin);
                            }
                        }
                    }
                }

                if !submitted && params.get("form").and_then(Value::as_object).is_some() {
                    request = request.header("content-type", "application/x-www-form-urlencoded");
                }

                if params.get("json").map(|value| !value.is_null()).unwrap_or(false) {
                    request = request.header("content-type", "application/json");
                }

                if let Some(found) = header {
                    request = request.header("akamai-bm-telemetry", found);
                }

                if !request.headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("cookie")) {
                    let cookies = session.jar().header(&url);
                    if !cookies.is_empty() {
                        request = request.header("cookie", cookies);
                    }
                }

                if let Some(headers) = params.get("headers").and_then(Value::as_object) {
                    for (name, value) in headers {
                        if let Some(text) = value.as_str() {
                            request = request.header(name.clone(), text.to_string());
                        }
                    }
                }

                let response = session.fetch(request)?;
                let text = response.text();

                Ok(json!({
                    "status": response.status,
                    "url": response.url,
                    "headers": response
                        .headers
                        .iter()
                        .filter(|(name, _)| !name.eq_ignore_ascii_case("set-cookie"))
                        .map(|(name, value)| json!([name, value]))
                        .collect::<Vec<_>>(),
                    "body": text,
                    "cookies": session.cookies(),
                    "refused": refused(response.status, &text),
                }))
            }

            "relay" => {
                let url = params
                    .get("url")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| ClientError::bad_input("relay needs a url"))?;

                let method = params
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or("GET")
                    .to_uppercase();

                let body = match params.get("body_b64").and_then(Value::as_str) {
                    Some(encoded) if !encoded.is_empty() => Some(
                        STANDARD.decode(encoded).map_err(|error| {
                            ClientError::bad_input(format!("body_b64 is not base64: {error}"))
                        })?,
                    ),
                    _ => None,
                };

                let mut headers = Vec::new();
                if let Some(pairs) = params.get("headers").and_then(Value::as_array) {
                    for pair in pairs {
                        let Some(entry) = pair.as_array() else { continue };
                        let (Some(name), Some(value)) =
                            (entry.first().and_then(Value::as_str), entry.get(1).and_then(Value::as_str))
                        else {
                            continue;
                        };
                        headers.push((name.to_string(), value.to_string()));
                    }
                }

                let order: Vec<String> = headers.iter().map(|(name, _)| name.clone()).collect();
                let session = self.session()?;

                let response = session.fetch(FetchRequest {
                    url,
                    method,
                    headers,
                    body,
                    fingerprint: None,
                    order,
                })?;

                Ok(json!({
                    "status": response.status,
                    "url": response.url,
                    "headers": response
                        .headers
                        .iter()
                        .map(|(name, value)| json!([name, value]))
                        .collect::<Vec<_>>(),
                    "body_b64": STANDARD.encode(&response.body),
                    "cookies": session.cookies(),
                }))
            }

            "page" => {
                let session = self.session()?;
                let html = session.html().to_string();

                Ok(json!({
                    "url": session.page_url(),
                    "html": html,
                    "fields": form_fields(&html),
                    "bytes": html.len(),
                }))
            }

            "cookies" => {
                if let Some(header) = params.get("set").and_then(Value::as_str) {
                    let url = self.config.page_url.clone().unwrap_or_default();

                    for pair in header.split(';') {
                        let trimmed = pair.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        let _ = self.jar.add(&url, trimmed);
                    }
                }

                let session = self.session()?;
                let pairs = session.cookie_pairs();

                Ok(json!({
                    "header": pairs
                        .iter()
                        .map(|(name, value)| format!("{name}={value}"))
                        .collect::<Vec<_>>()
                        .join("; "),
                    "summary": cookies::summarise(&pairs),
                    "names": pairs.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>(),
                }))
            }

            "pow" => {
                let rounds = params
                    .get("rounds")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize)
                    .unwrap_or(pow::ROUNDS);

                let workers = if self.config.workers == 0 { 4 } else { self.config.workers };
                let ceiling = self.config.max_attempts;

                let start_ts = match params.get("start_ts").and_then(Value::as_u64) {
                    Some(found) => Some(found),
                    None => match self.session.as_mut() {
                        Some(session) if session.is_open() => session.start_ts()?,
                        _ => None,
                    },
                };

                let challenge = match params.get("challenge").and_then(Value::as_str) {
                    Some(text) => pow::parse_challenge(text),
                    None => {
                        let cookie = match params.get("abck").and_then(Value::as_str) {
                            Some(found) => found.to_string(),
                            None => self
                                .session()?
                                .cookie_pairs()
                                .into_iter()
                                .find(|(name, _)| name == "_abck")
                                .map(|(_, value)| value)
                                .ok_or_else(|| {
                                    ClientError::bad_input("no _abck cookie, pass abck or challenge")
                                })?,
                        };

                        pow::from_abck(&cookie).into_iter().next()
                    }
                };

                let challenge = challenge.ok_or_else(|| {
                    ClientError::unsupported("no proof of work is being asked for")
                })?;

                let start_ts = start_ts.ok_or_else(|| {
                    ClientError::bad_input("no start_ts, pass one or solve a page first")
                })?;

                let answer = pow::solve_rounds(&challenge, start_ts, rounds, ceiling, workers)
                    .map_err(|error| ClientError::internal(error.to_string()))?;

                Ok(json!({
                    "challenge": challenge,
                    "prefix": answer.prefix,
                    "nonces": answer.nonces,
                    "attempts": answer.attempts,
                    "elapsed_ms": answer.elapsed_ms,
                    "answer": answer.formatted,
                }))
            }

            "pixel" => {
                let session = self.open_session()?;
                Ok(json!({ "pixel": session.run_pixel()? }))
            }

            "script" => {
                let session = self.open_session()?;
                let (url, source) = session.sensor_source();

                Ok(json!({ "url": url, "bytes": source.len(), "source": source }))
            }

            "eval" => {
                let expression = params
                    .get("expression")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ClientError::bad_input("eval needs an expression"))?
                    .to_string();
                let nudge = params.get("nudge_ms").and_then(Value::as_f64);

                let session = self.open_session()?;
                if let Some(ms) = nudge {
                    session.nudge(ms)?;
                }

                match session.eval(&expression) {
                    Ok(value) => Ok(json!({ "value": value })),
                    Err(error) => Ok(json!({ "value": Value::Null, "error": error.to_string() })),
                }
            }

            "reset" => {
                let clear = params.get("cookies").and_then(Value::as_bool).unwrap_or(false);

                if let Some(session) = self.session.as_mut() {
                    session.close();
                }
                self.session = None;

                if clear {
                    self.jar.clear();
                }

                Ok(json!({ "open": false }))
            }

            other => Err(ClientError::unsupported(format!("{ID} has no op {other}"))),
        }
    }

    fn health(&mut self) -> ClientResult<Value> {
        Ok(json!({
            "ok": true,
            "profile": self.record.id,
            "builtin_profile": self.record.id == BUILTIN_ID,
            "open": self.session.as_ref().map(Session::is_open).unwrap_or(false),
            "cookies": self.jar.names(),
        }))
    }

    fn diagnostics(&mut self) -> Value {
        let Some(session) = self.session.as_ref() else {
            return json!({ "open": false, "run": self.last_run });
        };

        json!({
            "open": session.is_open(),
            "run": self.last_run,
            "page": session.page_url(),
            "profile": session.profile_id(),
            "posts": session.posts(),
            "requests": session.requests().len(),
            "misses": session.misses(),
        })
    }

    fn close(&mut self) -> ClientResult<()> {
        if let Some(session) = self.session.as_mut() {
            session.close();
        }
        self.session = None;
        Ok(())
    }
}
