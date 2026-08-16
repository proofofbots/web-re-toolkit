pub mod discover;
pub mod pow;
pub mod report;
pub mod session;
pub mod token;

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use serde_json::{Value, json};

use wre_client::client::{Client, Registration};
use wre_client::context::{Call, Ctx, FetchRequest, HttpOptions, Jar};
use wre_client::error::{ClientError, ClientResult};
use wre_client::shape::{Shape, field};
use wre_client::spec::{Capabilities, ClientDescriptor, Concurrency, EventSpec, OpSpec};
use wre_sandbox::graph::{GraphLibrary, GraphProfile};

use session::{Session, Settings};

pub const ID: &str = "kasada";

const NOTES: &str = r#"## How a run works

`solve` is the whole flow. It fetches the url, and if the edge answers with the interrogation page instead of the page you asked for, it reads the `ips.js` the page names, runs that script unmodified in the sandbox, and lets the script post its own payload to `/tl`. The token the edge answers with comes back as `token`.

The token is bound to the `KP_UIDz` cookie the interstitial set, so solve against the url you actually want. `request` then sends your request through the same jar, transport fingerprint and user agent, which is the point.

`discover` is the cheap look: it fetches the page and reports the tenant path, the build version and whether an interrogation is being served, without running anything.

## The proof of work

Sites that turn it on want an `x-kpsdk-cd` header on every stamped request. Two ways to get one. `pow` computes it in Rust from a token and one of the loader's salts. `loader` mounts the site's own `p.js` in the same realm and `request` with `stamped` set sends through it, so the loader builds the header itself and a rebuild cannot drift from it.

## Sessions

A session is a client: one jar, one realm, one transport fingerprint. Keep it open across calls rather than opening one per call. `reset` drops the realm.

## Profiles

The interrogation enumerates the whole global surface, so this client mounts a graph profile: a captured object graph rather than a table of readings. Capture one with `wre sandbox capture --graph`, list them with `wre sandbox list`, then name one in `profile`. Without one, `solve` fails and says so; `discover`, `request` and `pow` still work, because none of them mount anything.

`misses` reports what the run asked for and the graph could not answer, separately from the receiver checks that fired on purpose."#;

pub fn registration() -> Registration {
    Registration {
        id: ID,
        describe,
        build,
    }
}

pub fn describe() -> ClientDescriptor {
    ClientDescriptor::new(ID, env!("CARGO_PKG_VERSION"))
        .summary("Runs a Kasada interrogation headlessly and carries the token the edge issues")
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
                        field("profile", Shape::Str).summary("Graph profile this session mounts"),
                        field("profiles", Shape::list(Shape::Str))
                            .summary("Every graph profile id the workspace holds"),
                        field("user_agent", Shape::Str),
                        field("fingerprint", Shape::Str)
                            .summary("Transport fingerprint, or where it is being derived from"),
                        field("open", Shape::Bool).summary("Whether a session is open"),
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
                            .summary("Whether the edge answered with an interrogation"),
                        field("surface", Shape::Json)
                            .summary("Tenant path, build version, the agent script and whether the page configures endpoints"),
                        field("cookies", Shape::Json).summary("Jar after the fetch"),
                    ],
                ),
            )
            .summary("Report the Kasada wiring of a page without running anything")
            .deadline_ms(45_000),
        )
        .op(
            OpSpec::new(
                "solve",
                Shape::object(
                    "SolveInput",
                    [
                        field("url", Shape::optional(Shape::Str))
                            .summary("Page to solve for, defaults to page_url from the config"),
                        field("wait_ms", Shape::optional(Shape::Int))
                            .summary("Overrides how long the agent is left running"),
                    ],
                ),
                Shape::object(
                    "Solved",
                    [
                        field("verdict", Shape::Str)
                            .summary("solved, unsolved or none"),
                        field("token", Shape::optional(Shape::Str))
                            .summary("The x-kpsdk-ct the edge issued"),
                        field("clearance", Shape::optional(Shape::Str))
                            .summary("The x-kpsdk-r the edge answered with"),
                        field("url", Shape::Str),
                        field("agent", Shape::Json).summary("The script that ran, and its size"),
                        field("payload_bytes", Shape::Int)
                            .summary("Size of the body the agent posted to /tl"),
                        field("sent", Shape::Json).summary("Every request the sandbox made"),
                        field("misses", Shape::list(Shape::Str))
                            .summary("Surfaces the run asked for that the graph could not answer"),
                        field("cookies", Shape::Json),
                        field("ms", Shape::Int),
                    ],
                ),
            )
            .summary("Run the interrogation and carry the token the edge issues")
            .deadline_ms(120_000)
            .streams(&["progress"]),
        )
        .op(
            OpSpec::new(
                "request",
                Shape::object(
                    "RequestInput",
                    [
                        field("url", Shape::Str),
                        field("method", Shape::Str).with_default(json!("GET")),
                        field("headers", Shape::optional(Shape::map(Shape::Str))),
                        field("body", Shape::optional(Shape::Str)),
                        field("token", Shape::Bool)
                            .summary("Carry the solved token as headers and as the KP_UIDz cookie")
                            .with_default(json!(true)),
                        field("stamped", Shape::Bool)
                            .summary("Send from inside the realm so the mounted loader stamps it, proof of work included")
                            .with_default(json!(false)),
                    ],
                ),
                Shape::object(
                    "Answered",
                    [
                        field("status", Shape::Int),
                        field("url", Shape::Str),
                        field("headers", Shape::map(Shape::Str)),
                        field("bytes", Shape::Int),
                        field("body", Shape::Str),
                    ],
                ),
            )
            .summary("Send a request carrying the session's token, cookies and transport")
            .deadline_ms(60_000),
        )
        .op(
            OpSpec::new(
                "loader",
                Shape::object(
                    "LoaderInput",
                    [field("endpoints", Shape::optional(Shape::list(Shape::Json)))
                        .summary("KPSDK.configure entries, defaults to every path on the page's host")],
                ),
                Shape::object(
                    "Loaded",
                    [
                        field("url", Shape::Str),
                        field("bytes", Shape::Int),
                        field("threw", Shape::optional(Shape::Str)),
                    ],
                ),
            )
            .summary("Mount the site's own p.js so it stamps requests the way a page does")
            .deadline_ms(45_000),
        )
        .op(
            OpSpec::new(
                "pow",
                Shape::object(
                    "PowInput",
                    [
                        field("salt", Shape::Str)
                            .summary("One of the 64 character hex constants in the loader"),
                        field("token", Shape::optional(Shape::Str))
                            .summary("Token to bind to, defaults to the solved one"),
                        field("difficulty", Shape::Float).with_default(json!(10.0)),
                        field("count", Shape::Int).with_default(json!(2)),
                        field("st", Shape::optional(Shape::Int))
                            .summary("Server clock sample, defaults to the one /tl answered with"),
                    ],
                ),
                Shape::object(
                    "Proof",
                    [
                        field("header", Shape::Str).summary("Ready for x-kpsdk-cd"),
                        field("answers", Shape::list(Shape::Int)),
                        field("work_time", Shape::Int),
                        field("id", Shape::Str),
                        field("duration", Shape::Float),
                    ],
                ),
            )
            .summary("Build an x-kpsdk-cd header for a token")
            .deadline_ms(30_000),
        )
        .op(
            OpSpec::new(
                "payload",
                Shape::object("PayloadInput", []),
                Shape::object(
                    "Payload",
                    [
                        field("bytes", Shape::Int),
                        field("body", Shape::optional(Shape::Str))
                            .summary("The /tl body the agent built, base64"),
                    ],
                ),
            )
            .summary("The encrypted body the agent posted"),
        )
        .op(
            OpSpec::new(
                "vector",
                Shape::object("VectorInput", []),
                Shape::object(
                    "Vector",
                    [
                        field("slots", Shape::Int),
                        field("vector", Shape::Json)
                            .summary("The signal array, as the agent built it"),
                        field("agent", Shape::Str)
                            .summary("The build that produced it, so a replay lines up slot for slot"),
                    ],
                ),
            )
            .summary("The signal vector behind the payload, when capture_vector is on"),
        )
        .op(
            OpSpec::new(
                "report",
                Shape::object("ReportInput", []),
                Shape::object(
                    "Report",
                    [
                        field("posted", Shape::Int)
                            .summary("How many self reports the agent tried to send"),
                        field("about", Shape::Json).summary("Build tag, version and origin"),
                        field("flagged", Shape::Json)
                            .summary("Checks the agent flagged, with what each said"),
                        field("raw", Shape::Json).summary("The decoded report in full"),
                    ],
                ),
            )
            .summary("Decode the report the agent writes about itself"),
        )
        .op(
            OpSpec::new(
                "cookies",
                Shape::object("CookiesInput", []),
                Shape::object(
                    "Cookies",
                    [
                        field("pairs", Shape::map(Shape::Str)),
                        field("header", Shape::Str).summary("Ready for a Cookie header"),
                    ],
                ),
            )
            .summary("Read the session jar"),
        )
        .op(
            OpSpec::new(
                "misses",
                Shape::object("MissesInput", []),
                Shape::object(
                    "Misses",
                    [
                        field("misses", Shape::list(Shape::Str))
                            .summary("Asked for and not answered"),
                        field("guards", Shape::list(Shape::Str))
                            .summary("Receiver checks that fired, which is a browser's answer too"),
                    ],
                ),
            )
            .summary("What the sandbox could not answer, and what it refused on purpose"),
        )
        .op(
            OpSpec::new(
                "reset",
                Shape::object(
                    "ResetInput",
                    [field("cookies", Shape::Bool)
                        .summary("Empty the jar as well")
                        .with_default(json!(false))],
                ),
                Shape::object("Reset", [field("ok", Shape::Bool)]),
            )
            .summary("Drop the realm and start a new session"),
        )
        .event(
            EventSpec::new(
                "progress",
                Shape::object(
                    "Progress",
                    [
                        field("done", Shape::Int),
                        field("total", Shape::Int),
                        field("note", Shape::Str),
                    ],
                ),
            )
            .summary("Where a solve has got to"),
        )
}

fn config_shape() -> Shape {
    Shape::object(
        "KasadaConfig",
        [
            field("page_url", Shape::optional(Shape::Str))
                .summary("Page the session solves for when an op does not name one"),
            field("profile", Shape::optional(Shape::Str))
                .summary("Graph profile id, from `wre sandbox list`"),
            field("proxy", Shape::optional(Shape::Str))
                .summary("Proxy url the session and the sandbox both go through, http or socks5"),
            field("fingerprint", Shape::optional(Shape::Str))
                .summary("Transport fingerprint as profile[:platform], defaults to the sandbox profile's user agent"),
            field("user_agent", Shape::optional(Shape::Str))
                .summary("Overrides the user agent the sandbox profile carries"),
            field("wait_ms", Shape::Int)
                .summary("How long to let the agent run before giving up on a token")
                .with_default(json!(20_000)),
            field("step_ms", Shape::Int)
                .summary("How often the timer queue is drained while the agent runs")
                .with_default(json!(100)),
            field("paced", Shape::Bool)
                .summary("Spend the wait in real time so the payload's clock matches the edge's")
                .with_default(json!(true)),
            field("friction_ms", Shape::Float)
                .summary("Virtual cost of one DOM operation")
                .with_default(json!(0.12)),
            field("report", Shape::Bool)
                .summary("Let the agent's self report reach reporting.cdndex.io. Off by default: it is held back and decoded locally, which is what the report op reads")
                .with_default(json!(false)),
            field("version", Shape::Str)
                .summary("Build version to claim when the page names none")
                .with_default(json!("j-1.2.661")),
            field("timeout_ms", Shape::Int)
                .summary("Cap on one http request the session makes")
                .with_default(json!(90_000)),
            field("frames", Shape::Int)
                .summary("Child realms opened up front for the iframes the agent creates")
                .with_default(json!(4)),
            field("capture_vector", Shape::Bool)
                .summary("Keep the signal vector the agent built before it sealed it, for the vector op")
                .with_default(json!(false)),
            field("seed", Shape::optional(Shape::Int))
                .summary("Seed the random source"),
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
    proxy: Option<String>,
    #[serde(default)]
    fingerprint: Option<String>,
    #[serde(default)]
    user_agent: Option<String>,
    #[serde(default = "default_wait")]
    wait_ms: u64,
    #[serde(default = "default_step")]
    step_ms: u64,
    #[serde(default = "yes")]
    paced: bool,
    #[serde(default = "default_friction")]
    friction_ms: f64,
    #[serde(default)]
    report: bool,
    #[serde(default = "default_version")]
    version: String,
    #[serde(default = "default_timeout")]
    timeout_ms: u64,
    #[serde(default = "default_frames")]
    frames: usize,
    #[serde(default)]
    capture_vector: bool,
    #[serde(default)]
    seed: Option<u64>,
}

fn default_wait() -> u64 {
    20_000
}

fn default_step() -> u64 {
    100
}

fn default_friction() -> f64 {
    0.12
}

fn default_version() -> String {
    "j-1.2.661".to_string()
}

fn default_timeout() -> u64 {
    90_000
}

fn default_frames() -> usize {
    4
}

fn yes() -> bool {
    true
}

fn build(ctx: Ctx, config: Value) -> ClientResult<Box<dyn Client>> {
    let config: Config = serde_json::from_value(config)
        .map_err(|error| ClientError::bad_input(format!("config rejected: {error}")))?;

    let mut ctx = ctx;
    if let Some(seed) = config.seed {
        ctx = ctx.with_seed(seed);
    }

    ctx.fact("profile", json!(config.profile));
    ctx.fact("page_url", json!(config.page_url));

    Ok(Box::new(Kasada {
        ctx,
        config,
        record: None,
        jar: Jar::new(),
        session: None,
    }))
}

fn resolve_profile(ctx: &Ctx, config: &Config) -> ClientResult<GraphProfile> {
    let Some(workspace) = ctx.workspace() else {
        return Err(ClientError::resource(
            "no workspace on disk, so there is no graph profile to mount; run wre sandbox capture --graph",
        ));
    };

    let library = GraphLibrary::load(workspace.join("profiles").join("graph"))
        .map_err(|error| ClientError::resource(format!("the profile library failed: {error}")))?;

    if library.is_empty() {
        return Err(ClientError::resource(
            "profiles/graph is empty; record one with wre sandbox capture --graph",
        ));
    }

    library
        .resolve(config.profile.as_deref())
        .map_err(|error| ClientError::bad_input(error.to_string()))
}

struct Kasada {
    ctx: Ctx,
    config: Config,
    record: Option<GraphProfile>,
    jar: Jar,
    session: Option<Session>,
}

impl Kasada {
    fn record(&mut self) -> ClientResult<&GraphProfile> {
        if self.record.is_none() {
            let found = resolve_profile(&self.ctx, &self.config)?;
            self.ctx.fact("profile", json!(found.id));
            self.ctx.fact("user_agent", json!(found.user_agent));
            self.record = Some(found);
        }

        Ok(self.record.as_ref().expect("profile"))
    }

    fn user_agent(&self) -> String {
        self.config
            .user_agent
            .clone()
            .filter(|value| !value.is_empty())
            .or_else(|| self.record.as_ref().map(|found| found.user_agent.clone()))
            .unwrap_or_default()
    }

    fn profiles(&self) -> Vec<String> {
        match self.ctx.workspace() {
            Some(workspace) => GraphLibrary::load(workspace.join("profiles").join("graph"))
                .map(|library| library.ids())
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }
    fn settings(&self) -> Settings {
        Settings {
            wait_ms: self.config.wait_ms as f64,
            step_ms: self.config.step_ms as f64,
            friction_ms: self.config.friction_ms,
            timeout_ms: self.config.timeout_ms,
            paced: self.config.paced,
            report: self.config.report,
            version: self.config.version.clone(),
            frames: self.config.frames,
            capture_vector: self.config.capture_vector,
        }
    }

    fn fresh(&mut self) -> ClientResult<Session> {
        if self.user_agent().is_empty() {
            self.record()?;
        }

        let record = self.record.clone();
        let user_agent = self.user_agent();

        if user_agent.is_empty() {
            return Err(ClientError::bad_input(
                "the graph profile carries no user agent, set user_agent in the config",
            ));
        }

        let mut options = HttpOptions::with_proxy(self.config.proxy.as_deref());
        options.fingerprint = self.config.fingerprint.clone();
        options.user_agent = Some(user_agent.clone());
        options.timeout_secs = Some(self.config.timeout_ms.div_ceil(1000).max(1));
        options.jar = Some(self.jar.clone());

        let http = Arc::new(self.ctx.http_with(options)?);
        let id = record
            .as_ref()
            .map(|found| found.id.clone())
            .unwrap_or_default();

        Ok(Session::new(
            http,
            self.jar.clone(),
            record,
            id,
            self.settings(),
            user_agent,
        ))
    }

    fn mountable(&mut self) -> ClientResult<Session> {
        self.record()?;
        self.fresh()
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

    fn target(&self, params: &Value) -> ClientResult<String> {
        params
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| self.config.page_url.clone())
            .ok_or_else(|| ClientError::bad_input("no url, pass one or set page_url in the config"))
    }
}

impl Client for Kasada {
    fn call(&mut self, op: &str, params: Value, call: &Call) -> ClientResult<Value> {
        call.check()?;

        match op {
            "info" => {
                let profiles = self.profiles();
                let chosen = self
                    .record
                    .as_ref()
                    .map(|found| found.id.clone())
                    .or_else(|| self.config.profile.clone())
                    .or_else(|| profiles.first().cloned())
                    .unwrap_or_default();

                Ok(json!({
                    "target": ID,
                    "version": env!("CARGO_PKG_VERSION"),
                    "profile": chosen,
                    "profiles": profiles,
                    "user_agent": self.user_agent(),
                    "fingerprint": self.config.fingerprint.clone().unwrap_or_else(|| "from user agent".to_string()),
                    "open": self.session.as_ref().map(Session::is_open).unwrap_or(false),
                }))
            }

            "discover" => {
                let url = self.target(&params)?;
                let session = self.session()?;
                let response = session.navigate(&url)?;

                Ok(json!({
                    "url": session.page_url(),
                    "status": response.status,
                    "protected": session.surface().interstitial || session.surface().tenant.is_some(),
                    "surface": session.surface(),
                    "cookies": cookie_map(session),
                }))
            }

            "solve" => {
                let url = self.target(&params)?;

                if let Some(wait) = params.get("wait_ms").and_then(Value::as_u64) {
                    self.config.wait_ms = wait;
                }

                let mut session = self.mountable()?;
                call.progress(1, 3, "fetching the page");

                let run = session.open(&url);
                self.session = Some(session);

                let run = run?;
                call.progress(3, 3, "answered");

                let session = self.session.as_ref().expect("session");

                if session.verdict() != token::Verdict::Solved {
                    return Err(ClientError::blocked(format!(
                        "the edge answered {} to the interrogation",
                        session.verdict().as_str()
                    ))
                    .with_detail(run));
                }

                Ok(run)
            }

            "request" => {
                let url = params
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ClientError::bad_input("url is required"))?
                    .to_string();

                let method = params.get("method").and_then(Value::as_str).unwrap_or("GET").to_string();
                let body = params.get("body").and_then(Value::as_str).map(str::to_string);
                let carry = params.get("token").and_then(Value::as_bool).unwrap_or(true);
                let stamped = params.get("stamped").and_then(Value::as_bool).unwrap_or(false);
                let extra = params.get("headers").cloned().unwrap_or(Value::Null);

                if stamped {
                    let session = self.open_session()?;

                    if !session.loaded() {
                        return Err(ClientError::bad_input(
                            "no loader is mounted, call loader before a stamped request",
                        ));
                    }

                    return session.stamped(&url, &method, &extra, body.as_deref());
                }

                let version = self.session.as_ref().map(|held| held.version().to_string());
                let issued = self.session.as_ref().and_then(Session::issued);
                let user_agent = self.user_agent();
                let session = self.session()?;

                let mut request = match &body {
                    Some(found) => FetchRequest::post(url.clone(), found.clone().into_bytes()),
                    None => FetchRequest::get(url.clone()),
                };

                request = request
                    .header("accept", "*/*")
                    .header("accept-language", "en-US,en;q=0.9")
                    .header("user-agent", user_agent.clone());

                for (name, value) in session::client_hints(&user_agent) {
                    request = request.header(name, value);
                }

                if method.to_uppercase() != "GET" {
                    request.method = method.to_uppercase();
                }

                if carry
                    && let Some(found) = &issued
                {
                    for (name, value) in
                        token::headers(&found.token, version.as_deref().unwrap_or("j-1.2.661"), None)
                    {
                        request = request.header(name, value);
                    }

                    let _ = session.jar().add(&url, &format!("{}={}", token::COOKIE, found.token));
                    let _ = session
                        .jar()
                        .add(&url, &format!("{}={}", token::SESSION_COOKIE, found.token));
                }

                if let Some(fields) = extra.as_object() {
                    for (name, value) in fields {
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
                        .map(|(name, value)| (name.to_lowercase(), Value::String(value.clone())))
                        .collect::<serde_json::Map<String, Value>>(),
                    "bytes": text.len(),
                    "body": text,
                }))
            }

            "loader" => {
                let entries = params.get("endpoints").cloned().unwrap_or(Value::Null);
                let session = self.open_session()?;

                let entries = match entries {
                    Value::Array(found) => Value::Array(found),
                    _ => {
                        let host = url::Url::parse(session.page_url())
                            .ok()
                            .and_then(|parsed| parsed.host_str().map(str::to_string))
                            .unwrap_or_default();

                        json!([{ "method": "*", "domain": host, "path": "/" }])
                    }
                };

                session.load_loader(&entries)
            }

            "pow" => {
                let salt = params
                    .get("salt")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ClientError::bad_input("salt is required"))?
                    .to_string();

                let issued = self.session.as_ref().and_then(Session::issued);

                let ct = params
                    .get("token")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| issued.as_ref().map(|found| found.token.clone()))
                    .ok_or_else(|| ClientError::bad_input("no token, pass one or call solve first"))?;

                let difficulty = params
                    .get("difficulty")
                    .and_then(Value::as_f64)
                    .unwrap_or(pow::DIFFICULTY);
                let count = params
                    .get("count")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize)
                    .unwrap_or(pow::COUNT);

                let st = params
                    .get("st")
                    .and_then(Value::as_i64)
                    .or_else(|| issued.as_ref().and_then(|found| found.server_time))
                    .unwrap_or_default();
                let rst = issued.as_ref().map(|found| found.received_at).unwrap_or_default();

                let request = pow::Request {
                    ct,
                    salt,
                    id: self.ctx.random_hex(16),
                    work_time: self.ctx.now_ms() as i64,
                    difficulty,
                    count,
                    extra: String::new(),
                    st,
                    rst: if st == 0 { 0 } else { rst },
                };

                let proof = pow::build(&request)
                    .ok_or_else(|| ClientError::internal("the proof of work did not converge"))?;

                Ok(json!({
                    "header": proof.header(),
                    "answers": proof.answers,
                    "work_time": proof.work_time,
                    "id": proof.id,
                    "duration": proof.duration,
                }))
            }

            "payload" => {
                let session = self.open_session()?;
                let payload = session.payload();

                Ok(json!({
                    "bytes": payload.as_ref().map(Vec::len).unwrap_or_default(),
                    "body": payload.map(|bytes| STANDARD.encode(bytes)),
                }))
            }

            "vector" => {
                let session = self.open_session()?;
                let vector = session.vector()?;
                let agent = session.agent_source().to_string();

                Ok(json!({
                    "slots": vector.as_array().map(Vec::len).unwrap_or_default(),
                    "vector": vector,
                    "agent": agent,
                }))
            }

            "report" => {
                let session = self.open_session()?;
                let reports = session.reports();

                let richest = reports
                    .iter()
                    .max_by_key(|entry| entry.as_object().map(|fields| fields.len()).unwrap_or(0))
                    .cloned();

                Ok(json!({
                    "posted": reports.len(),
                    "about": richest.as_ref().map(report::about).unwrap_or(Value::Null),
                    "flagged": richest.as_ref().map(report::flagged).unwrap_or_default(),
                    "raw": richest.unwrap_or(Value::Null),
                }))
            }

            "cookies" => {
                let session = self.session()?;
                let pairs = session.cookie_pairs();

                Ok(json!({
                    "pairs": pairs
                        .iter()
                        .map(|(name, value)| (name.clone(), Value::String(value.clone())))
                        .collect::<serde_json::Map<String, Value>>(),
                    "header": pairs
                        .iter()
                        .map(|(name, value)| format!("{name}={value}"))
                        .collect::<Vec<_>>()
                        .join("; "),
                }))
            }

            "misses" => {
                let session = self.open_session()?;

                Ok(json!({
                    "misses": session.misses(),
                    "guards": session.guards(),
                }))
            }

            "reset" => {
                if let Some(session) = self.session.as_mut() {
                    session.close();
                }

                self.session = None;

                if params.get("cookies").and_then(Value::as_bool).unwrap_or(false) {
                    self.jar.clear();
                }

                Ok(json!({ "ok": true }))
            }

            other => Err(ClientError::unsupported(format!("{ID} has no op {other}"))),
        }
        .map_err(|error| error.with_op(op).with_target(ID))
    }

    fn health(&mut self) -> ClientResult<Value> {
        Ok(json!({
            "ok": true,
            "target": ID,
            "detail": {
                "open": self.session.as_ref().map(Session::is_open).unwrap_or(false),
                "profile": self.record.as_ref().map(|found| found.id.clone()),
            },
        }))
    }

    fn diagnostics(&mut self) -> Value {
        let misses = self
            .session
            .as_mut()
            .map(Session::misses)
            .unwrap_or_default();
        let session = self.session.as_ref();

        json!({
            "profile": self.record.as_ref().map(|found| found.id.clone()),
            "user_agent": self.user_agent(),
            "proxy_set": self.config.proxy.is_some(),
            "page_url": session.map(Session::page_url),
            "verdict": session.map(|held| held.verdict().as_str()),
            "misses": misses,
            "sent": session.map(Session::sent),
        })
    }

    fn close(&mut self) -> ClientResult<()> {
        if let Some(session) = self.session.as_mut() {
            session.close();
        }

        Ok(())
    }
}

fn cookie_map(session: &Session) -> Value {
    Value::Object(
        session
            .cookie_pairs()
            .into_iter()
            .map(|(name, value)| (name, Value::String(value)))
            .collect(),
    )
}
