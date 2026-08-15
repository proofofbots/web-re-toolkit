use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use wre_client::client::{Client, Registration};
use wre_client::context::{Call, Clock, Ctx, FetchRequest};
use wre_client::error::{ClientError, ClientResult};
use wre_client::shape::{Shape, field};
use wre_client::spec::{Capabilities, ClientDescriptor, Concurrency, EventSpec, OpSpec};
use wre_js::pipeline::SourceKind;
use wre_live::mount::{Mount, MountPlan, mount};
use wre_live::realm::{RealmOptions, initialize};

pub const ID: &str = "example";

const SCRIPT: &str = include_str!("../assets/collect.js");
const DEFAULT_KEY: &str = "example";
const HEADER: &str = "x-signature";

pub fn registration() -> Registration {
    Registration { id: ID, describe, build }
}

pub fn describe() -> ClientDescriptor {
    ClientDescriptor::new(ID, env!("CARGO_PKG_VERSION"))
        .summary("Seals a payload with the demo collector's own primitives")
        .capabilities(Capabilities {
            needs_v8: true,
            needs_chrome: false,
            needs_network: true,
            stateful: true,
            concurrency: Concurrency::PerSession,
            warmup_ms: 150,
        })
        .config(config_shape())
        .op(
            OpSpec::new("roles", Shape::object("RolesInput", []), Shape::list(Shape::Str))
                .summary("Which primitives were found in the mounted build"),
        )
        .op(
            OpSpec::new(
                "build",
                Shape::object("BuildInput", []),
                Shape::object(
                    "BuildInfo",
                    [
                        field("build", Shape::Str),
                        field("source_sha", Shape::Str),
                        field("roles", Shape::list(Shape::Str)),
                    ],
                ),
            )
            .summary("The build tag and source digest this session mounted"),
        )
        .op(
            OpSpec::new(
                "hash",
                Shape::object("HashInput", [field("text", Shape::Str)]),
                Shape::object(
                    "HashOutput",
                    [field("value", Shape::Int), field("hex", Shape::Str)],
                ),
            )
            .summary("Run the target's own string hash"),
        )
        .op(
            OpSpec::new(
                "encode",
                Shape::object("EncodeInput", [field("value", Shape::Json)]),
                Shape::object(
                    "EncodeOutput",
                    [field("text", Shape::Str), field("bytes", Shape::Int)],
                ),
            )
            .summary("Run the target's own json encoder"),
        )
        .op(
            OpSpec::new(
                "seal",
                Shape::object(
                    "SealInput",
                    [
                        field("value", Shape::Json),
                        field("key", Shape::optional(Shape::Str))
                            .summary("Overrides the key seed from the session config"),
                    ],
                ),
                Shape::object(
                    "SealOutput",
                    [
                        field("body", Shape::Str),
                        field("digest", Shape::Str),
                        field("bytes", Shape::Int),
                    ],
                ),
            )
            .summary("Encode and seal a value into a wire body"),
        )
        .op(
            OpSpec::new("payload", facts_shape(), Shape::Json)
                .summary("Build the payload object the collector would send"),
        )
        .op(
            OpSpec::new(
                "solve",
                Shape::reference("Facts"),
                Shape::object(
                    "Solved",
                    [
                        field("body", Shape::Str),
                        field("digest", Shape::Str),
                        field("headers", Shape::map(Shape::Str)),
                        field("build", Shape::Str),
                        field("took_ms", Shape::Int),
                    ],
                ),
            )
            .summary("Build, encode and seal a payload in one call")
            .deadline_ms(20_000)
            .streams(&["progress"]),
        )
        .op(
            OpSpec::new(
                "stall",
                Shape::object(
                    "StallInput",
                    [field("ms", Shape::Int)
                        .summary("How long to block for")
                        .with_default(json!(100))],
                ),
                Shape::object("StallOutput", [field("slept_ms", Shape::Int)]),
            )
            .summary("Block so deadline and cancel paths can be exercised")
            .deadline_ms(60_000),
        )
        .op(
            OpSpec::new(
                "submit",
                Shape::object(
                    "SubmitInput",
                    [
                        field("url", Shape::Str),
                        field("body", Shape::Str),
                        field("headers", Shape::optional(Shape::map(Shape::Str))),
                    ],
                ),
                Shape::object(
                    "SubmitOutput",
                    [
                        field("status", Shape::Int),
                        field("ok", Shape::Bool),
                        field("body_sha", Shape::Str),
                    ],
                ),
            )
            .summary("Post a sealed body through the host's http client")
            .deadline_ms(30_000),
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
            .summary("Step counter for solve"),
        )
        .event(
            EventSpec::new(
                "log",
                Shape::object("LogLine", [field("level", Shape::Str), field("text", Shape::Str)]),
            )
            .summary("Free text from the client"),
        )
}

fn config_shape() -> Shape {
    Shape::object(
        "ExampleConfig",
        [
            field("script", Shape::optional(Shape::Str))
                .summary("Script source to mount, overrides the bundled copy"),
            field("script_path", Shape::optional(Shape::Str))
                .summary("Path to a script to mount, read by the host process"),
            field("key", Shape::Str)
                .summary("Seed the collector derives its seal table from")
                .with_default(json!(DEFAULT_KEY)),
            field("expect_build", Shape::optional(Shape::Str))
                .summary("Fail with target_drift when the mounted build tag differs"),
            field("clock_ms", Shape::optional(Shape::Int))
                .summary("Freeze the realm clock for reproducible payloads"),
            field("seed", Shape::optional(Shape::Int))
                .summary("Seed the realm random source"),
            field("proxy", Shape::optional(Shape::Str))
                .summary("Proxy used by submit, scheme://user:pass@host:port"),
            field("tolerate_throw", Shape::Bool).with_default(json!(true)),
            field("timeout_ms", Shape::Int).with_default(json!(30_000)),
        ],
    )
}

fn facts_shape() -> Shape {
    Shape::object(
        "Facts",
        [
            field("url", Shape::Str),
            field("title", Shape::optional(Shape::Str)),
            field("width", Shape::Int).with_default(json!(1920)),
            field("height", Shape::Int).with_default(json!(1080)),
            field("depth", Shape::Int).with_default(json!(24)),
            field("language", Shape::Str).with_default(json!("en-US")),
            field("timezone", Shape::Str).with_default(json!("UTC")),
            field("webdriver", Shape::Bool).with_default(json!(false)),
            field("extra", Shape::optional(Shape::map(Shape::Json))),
        ],
    )
}

#[derive(Debug, Clone, Deserialize)]
struct Config {
    #[serde(default)]
    script: Option<String>,
    #[serde(default)]
    script_path: Option<String>,
    #[serde(default = "default_key")]
    key: String,
    #[serde(default)]
    expect_build: Option<String>,
    #[serde(default)]
    clock_ms: Option<f64>,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default)]
    proxy: Option<String>,
    #[serde(default = "default_true")]
    tolerate_throw: bool,
    #[serde(default = "default_timeout")]
    timeout_ms: u64,
}

fn default_key() -> String {
    DEFAULT_KEY.to_string()
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    30_000
}

fn build(ctx: Ctx, config: Value) -> ClientResult<Box<dyn Client>> {
    let config: Config = serde_json::from_value(config)
        .map_err(|error| ClientError::bad_input(format!("config rejected: {error}")))?;
    Ok(Box::new(Example::open(ctx, config)?))
}

struct Example {
    ctx: Ctx,
    config: Config,
    mount: Mount,
    source_sha: String,
    build_tag: String,
    solved: u64,
    last_body_bytes: usize,
    last_payload_keys: Vec<String>,
}

impl Example {
    fn open(ctx: Ctx, config: Config) -> ClientResult<Self> {
        initialize();

        let mut ctx = ctx;
        if let Some(ms) = config.clock_ms {
            ctx = ctx.with_clock(Clock::Fixed(ms as u64));
        }
        if let Some(seed) = config.seed {
            ctx = ctx.with_seed(seed);
        }

        let source = match (&config.script, &config.script_path) {
            (Some(inline), _) => inline.clone(),
            (None, Some(path)) => std::fs::read_to_string(path).map_err(|error| {
                ClientError::resource(format!("script {path} did not open: {error}"))
            })?,
            (None, None) => SCRIPT.to_string(),
        };

        let source_sha = wre_core::digest::sha256_short(source.as_bytes());

        let plan = MountPlan {
            tolerate_throw: config.tolerate_throw,
            source_kind: SourceKind::Script,
            ..MountPlan::default()
        }
        .with_signature("hash", "0x811c9dc5|2166136261")
        .with_signature("seal", "\\^\\s*\\w+\\[\\s*\\w+\\s*%\\s*\\w+\\s*\\]")
        .with_signature("encodeJson", "\"Recursive input\"")
        .with_export("payload", "globalThis.__internals && __internals.codec.payload");

        let options = RealmOptions {
            timeout: Duration::from_millis(config.timeout_ms.max(100)),
            clock_ms: config.clock_ms,
            random_seed: config.seed,
            timers: true,
            codecs: true,
            heap_limit_mb: Some(256),
        };

        let mut mounted = mount(&source, &plan, options)
            .map_err(|error| ClientError::internal(format!("mount failed: {error}")))?;

        for role in ["hash", "seal", "encodeJson", "payload"] {
            if mounted.handles.get(role).is_none() {
                return Err(ClientError::drift(format!(
                    "role {role} was not found in this build, mounted roles are {}",
                    join(&mounted.roles())
                ))
                .with_detail(json!({ "source_sha": source_sha, "roles": mounted.roles() })));
            }
        }

        let build_tag = mounted
            .realm
            .eval_json("globalThis.__internals && __internals.build")
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "unknown".to_string());

        if let Some(expected) = &config.expect_build {
            if expected != &build_tag {
                return Err(ClientError::drift(format!(
                    "mounted build is {build_tag}, the client expects {expected}"
                ))
                .with_detail(json!({ "build": build_tag, "expected": expected })));
            }
        }

        ctx.count("example.session.opened");
        ctx.fact("build", json!(build_tag));
        ctx.fact("source_sha", json!(source_sha));
        ctx.fact("source_bytes", json!(source.len()));
        ctx.fact("roles", json!(mounted.roles()));
        ctx.fact("mount", json!({
            "patched": mounted.report.patched,
            "bytes": mounted.report.bytes,
            "roles": mounted.report.roles,
        }));

        Ok(Self {
            ctx,
            config,
            mount: mounted,
            source_sha,
            build_tag,
            solved: 0,
            last_body_bytes: 0,
            last_payload_keys: Vec::new(),
        })
    }

    fn hash_text(&mut self, text: &str) -> ClientResult<u64> {
        let value = self
            .mount
            .call("hash", &[json!(text)])
            .map_err(|error| ClientError::internal(format!("hash failed: {error}")))?;

        value
            .as_f64()
            .map(|number| number as u64)
            .ok_or_else(|| ClientError::drift(format!("hash returned {value}, not a number")))
    }

    fn seal_value(&mut self, value: &Value, key: &str) -> ClientResult<(String, String, usize)> {
        let text = self
            .mount
            .call("encodeJson", &[value.clone()])
            .map_err(|error| ClientError::internal(format!("encodeJson failed: {error}")))?;

        let text = text
            .as_str()
            .ok_or_else(|| ClientError::drift("encodeJson did not return a string"))?
            .to_string();

        let sealed = self
            .mount
            .call("seal", &[json!(text), json!(key)])
            .map_err(|error| ClientError::internal(format!("seal failed: {error}")))?;

        let bytes = to_bytes(&sealed)?;
        let body = wre_client::shape::encode_bytes(&bytes);
        let digest = self.hash_text(&body)?;

        Ok((body, format!("{digest:08x}"), bytes.len()))
    }

    fn facts_to_payload(&mut self, params: &Value) -> ClientResult<Value> {
        let mut facts = params.clone();
        if let Some(entries) = facts.as_object_mut() {
            entries.insert("now".to_string(), json!(self.ctx.now_ms()));
            entries.insert("nonce".to_string(), json!(self.ctx.random_hex(8)));
        }

        self.mount
            .call("payload", &[facts])
            .map_err(|error| ClientError::internal(format!("payload failed: {error}")))
    }
}

impl Client for Example {
    fn call(&mut self, op: &str, params: Value, call: &Call) -> ClientResult<Value> {
        call.check()?;
        let started = Instant::now();

        let outcome = match op {
            "roles" => Ok(json!(self.mount.roles())),

            "build" => Ok(json!({
                "build": self.build_tag,
                "source_sha": self.source_sha,
                "roles": self.mount.roles(),
            })),

            "hash" => {
                let text = string_at(&params, "text")?;
                let value = self.hash_text(&text)?;
                Ok(json!({ "value": value, "hex": format!("{value:08x}") }))
            }

            "encode" => {
                let value = params.get("value").cloned().unwrap_or(Value::Null);
                let text = self
                    .mount
                    .call("encodeJson", &[value])
                    .map_err(|error| ClientError::internal(format!("encodeJson failed: {error}")))?;
                let text = text
                    .as_str()
                    .ok_or_else(|| ClientError::drift("encodeJson did not return a string"))?
                    .to_string();
                Ok(json!({ "bytes": text.len(), "text": text }))
            }

            "seal" => {
                let value = params.get("value").cloned().unwrap_or(Value::Null);
                let key = params
                    .get("key")
                    .and_then(Value::as_str)
                    .unwrap_or(&self.config.key)
                    .to_string();
                let (body, digest, bytes) = self.seal_value(&value, &key)?;
                Ok(json!({ "body": body, "digest": digest, "bytes": bytes }))
            }

            "payload" => self.facts_to_payload(&params),

            "solve" => {
                call.progress(1, 3, "building payload");
                let payload = self.facts_to_payload(&params)?;

                let keys: Vec<String> = payload
                    .as_object()
                    .map(|entries| entries.keys().cloned().collect())
                    .unwrap_or_default();
                call.debug("payload", json!({ "keys": keys }));
                self.last_payload_keys = keys;

                call.check()?;
                call.progress(2, 3, "sealing");
                let key = self.config.key.clone();
                let (body, digest, bytes) = self.seal_value(&payload, &key)?;

                call.debug("sealed", json!({ "bytes": bytes, "digest": digest }));
                self.last_body_bytes = bytes;
                self.solved += 1;

                call.progress(3, 3, "done");
                self.ctx.count("example.solve");

                Ok(json!({
                    "body": body,
                    "digest": digest.clone(),
                    "headers": { HEADER: digest, "content-type": "application/octet-stream" },
                    "build": self.build_tag,
                    "took_ms": started.elapsed().as_millis() as u64,
                }))
            }

            "stall" => {
                let wanted = params.get("ms").and_then(Value::as_u64).unwrap_or(100);
                let until = Duration::from_millis(wanted);

                while started.elapsed() < until {
                    call.check()?;
                    std::thread::sleep(Duration::from_millis(5));
                }

                Ok(json!({ "slept_ms": started.elapsed().as_millis() as u64 }))
            }

            "submit" => {
                let url = string_at(&params, "url")?;
                let body = string_at(&params, "body")?;

                let mut request = FetchRequest::post(url.clone(), body.into_bytes());
                if let Some(headers) = params.get("headers").and_then(Value::as_object) {
                    for (name, value) in headers {
                        if let Some(text) = value.as_str() {
                            request = request.header(name.clone(), text.to_string());
                        }
                    }
                }

                let http = self.ctx.http(self.config.proxy.as_deref())?;
                let response = http.fetch(request)?;
                let ok = (200..300).contains(&response.status);

                if response.status == 403 || response.status == 429 {
                    return Err(ClientError::blocked(format!(
                        "{url} answered {}",
                        response.status
                    ))
                    .with_detail(json!({ "status": response.status })));
                }

                Ok(json!({
                    "status": response.status,
                    "ok": ok,
                    "body_sha": wre_core::digest::sha256_short(&response.body),
                }))
            }

            other => Err(ClientError::unsupported(format!("{ID} has no op {other}"))),
        };

        self.ctx
            .metric(&format!("example.{op}.ms"), started.elapsed().as_millis() as f64);

        outcome.map_err(|error| error.with_op(op).with_target(ID))
    }

    fn warmup(&mut self, _call: &Call) -> ClientResult<()> {
        self.hash_text("warmup").map(|_| ())
    }

    fn health(&mut self) -> ClientResult<Value> {
        let roles = self.mount.roles();
        let missing: Vec<&str> = ["hash", "seal", "encodeJson", "payload"]
            .into_iter()
            .filter(|role| !roles.iter().any(|found| found == role))
            .collect();

        Ok(json!({
            "ok": missing.is_empty(),
            "target": ID,
            "detail": {
                "build": self.build_tag,
                "source_sha": self.source_sha,
                "roles": roles,
                "missing": missing,
            }
        }))
    }

    fn diagnostics(&mut self) -> Value {
        let records = self.mount.realm.records().unwrap_or_default();

        let console: Vec<Value> = records
            .console
            .iter()
            .rev()
            .take(30)
            .map(|line| json!({ "level": line.level, "text": line.text }))
            .collect();

        let errors: Vec<Value> = records
            .errors
            .iter()
            .rev()
            .take(30)
            .map(|entry| json!({ "where": entry.where_, "text": entry.text }))
            .collect();

        let access: Vec<Value> = records
            .access
            .iter()
            .rev()
            .take(60)
            .map(|entry| json!({ "on": entry.on, "kind": entry.kind, "key": entry.key }))
            .collect();

        json!({
            "build": self.build_tag,
            "source_sha": self.source_sha,
            "roles": self.mount.roles(),
            "mount": {
                "roles": self.mount.report.roles,
                "patched": self.mount.report.patched,
                "bytes": self.mount.report.bytes,
            },
            "realm": {
                "heap_used": self.mount.realm.heap_used(),
                "console": console,
                "errors": errors,
                "access": access,
                "calls": records.calls.len(),
            },
            "state": {
                "solved": self.solved,
                "last_body_bytes": self.last_body_bytes,
                "last_payload_keys": self.last_payload_keys,
                "key_seed_sha": wre_core::digest::sha256_short(self.config.key.as_bytes()),
                "clock_ms": self.config.clock_ms,
                "seed": self.config.seed,
            },
        })
    }

    fn close(&mut self) -> ClientResult<()> {
        self.ctx.count("example.session.closed");
        Ok(())
    }
}

fn string_at(params: &Value, name: &str) -> ClientResult<String> {
    params
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ClientError::bad_input(format!("{name} is required and must be a string")))
}

fn to_bytes(value: &Value) -> ClientResult<Vec<u8>> {
    let items = value
        .as_array()
        .ok_or_else(|| ClientError::drift(format!("seal returned {value}, not an array")))?;

    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let byte = item
            .as_f64()
            .ok_or_else(|| ClientError::drift("seal returned a non numeric byte"))?;
        out.push(byte as u8);
    }
    Ok(out)
}

fn join(values: &[String]) -> String {
    if values.is_empty() { "none".to_string() } else { values.join(", ") }
}
