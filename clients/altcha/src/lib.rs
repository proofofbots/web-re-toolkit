pub mod challenge;
pub mod his;
pub mod obfuscation;
pub mod pow;
pub mod signature;

use std::time::Instant;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use wre_client::client::{Client, Registration};
use wre_client::context::{Call, Clock, Ctx, FetchRequest, Http, HttpOptions};
use wre_client::error::{ClientError, ClientResult};
use wre_client::shape::{Shape, decode_bytes, encode_bytes, field};
use wre_client::spec::{Capabilities, ClientDescriptor, Concurrency, EventSpec, OpSpec};

use challenge::Version;
use pow::{CounterMode, Parameters};

pub const ID: &str = "altcha";

const FIELD_NAME: &str = "altcha";

pub fn registration() -> Registration {
    Registration { id: ID, describe, build }
}

pub fn describe() -> ClientDescriptor {
    ClientDescriptor::new(ID, env!("CARGO_PKG_VERSION"))
        .summary("Solves ALTCHA proof-of-work challenges without a browser")
        .capabilities(Capabilities {
            needs_v8: false,
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
                        field("algorithms", Shape::list(Shape::Str)),
                        field("workers", Shape::Int),
                    ],
                ),
            )
            .summary("What this build supports"),
        )
        .op(
            OpSpec::new(
                "challenge",
                Shape::object(
                    "ChallengeInput",
                    [
                        field("url", Shape::optional(Shape::Str))
                            .summary("Overrides challenge_url from the session config"),
                        field("his", Shape::optional(Shape::Bool))
                            .summary("Answer a human interaction signature request"),
                    ],
                ),
                challenge_shape(),
            )
            .summary("Fetch a challenge, answering an interaction signature request if asked")
            .deadline_ms(30_000),
        )
        .op(
            OpSpec::new(
                "solve",
                Shape::object(
                    "SolveInput",
                    [
                        field("challenge", Shape::optional(Shape::Json))
                            .summary("A challenge object, otherwise one is fetched"),
                        field("url", Shape::optional(Shape::Str))
                            .summary("Challenge endpoint, overrides challenge_url"),
                        field("his", Shape::optional(Shape::Bool)),
                        field("max_counter", Shape::optional(Shape::Int)),
                        field("workers", Shape::optional(Shape::Int)),
                    ],
                ),
                Shape::object(
                    "Solved",
                    [
                        field("payload", Shape::Str)
                            .summary("Base64 form value for the altcha field"),
                        field("field", Shape::Str),
                        field("counter", Shape::Int),
                        field("derived_key", Shape::Str),
                        field("algorithm", Shape::Str),
                        field("format", Shape::Int).summary("Challenge format, 1 or 3"),
                        field("attempts", Shape::Int),
                        field("took_ms", Shape::Float),
                    ],
                ),
            )
            .summary("Fetch or take a challenge, solve it, and return the form payload")
            .deadline_ms(120_000)
            .streams(&["progress"]),
        )
        .op(
            OpSpec::new(
                "derive_key",
                Shape::object(
                    "DeriveKeyInput",
                    [
                        field("algorithm", Shape::Str),
                        field("nonce", Shape::Str).summary("Hex, or plain text in string mode"),
                        field("salt", Shape::Str).summary("Hex").with_default(json!("")),
                        field("counter", Shape::Int),
                        field("cost", Shape::Int).with_default(json!(1)),
                        field("key_length", Shape::Int).with_default(json!(32)),
                        field("memory_cost", Shape::optional(Shape::Int)),
                        field("parallelism", Shape::optional(Shape::Int)),
                        field("counter_mode", Shape::Str).with_default(json!("uint32")),
                    ],
                ),
                Shape::object(
                    "DerivedKey",
                    [field("key", Shape::Str), field("password", Shape::Str)],
                ),
            )
            .summary("Run one derivation, the unit the solver loops over"),
        )
        .op(
            OpSpec::new(
                "verify",
                Shape::object(
                    "VerifyInput",
                    [
                        field("payload", Shape::optional(Shape::Str)),
                        field("challenge", Shape::optional(Shape::Json)),
                        field("solution", Shape::optional(Shape::Json)),
                        field("secret", Shape::optional(Shape::Str))
                            .summary("HMAC secret, overrides hmac_secret from the config"),
                    ],
                ),
                Shape::object(
                    "Verified",
                    [
                        field("verified", Shape::Bool),
                        field("expired", Shape::Bool),
                        field("invalid_signature", Shape::optional(Shape::Bool)),
                        field("invalid_solution", Shape::optional(Shape::Bool)),
                        field("format", Shape::Int),
                    ],
                ),
            )
            .summary("Check a payload the way an altcha server does"),
        )
        .op(
            OpSpec::new(
                "create_challenge",
                Shape::object(
                    "CreateChallengeInput",
                    [
                        field("algorithm", Shape::Str).with_default(json!("SHA-256")),
                        field("cost", Shape::Int).with_default(json!(100_000)),
                        field("counter", Shape::optional(Shape::Int))
                            .summary("The counter the solver is meant to find"),
                        field("key_length", Shape::Int).with_default(json!(32)),
                        field("key_prefix_length", Shape::optional(Shape::Int)),
                        field("nonce", Shape::optional(Shape::Str)),
                        field("salt", Shape::optional(Shape::Str)),
                        field("expires_in_s", Shape::optional(Shape::Int)),
                        field("memory_cost", Shape::optional(Shape::Int)),
                        field("parallelism", Shape::optional(Shape::Int)),
                        field("secret", Shape::optional(Shape::Str)),
                        field("format", Shape::Int).with_default(json!(3)),
                    ],
                ),
                Shape::reference("Challenge"),
            )
            .summary("Build a signed challenge, for tests and for measuring solve cost")
            .deadline_ms(60_000),
        )
        .op(
            OpSpec::new(
                "his",
                Shape::object(
                    "HisInput",
                    [
                        field("width", Shape::Int).with_default(json!(1280)),
                        field("height", Shape::Int).with_default(json!(800)),
                        field("target_x", Shape::optional(Shape::Int)),
                        field("target_y", Shape::optional(Shape::Int)),
                        field("duration_ms", Shape::Int).with_default(json!(1400)),
                        field("start_ms", Shape::Int).with_default(json!(900)),
                        field("touch", Shape::Bool).with_default(json!(false)),
                        field("scroll", Shape::Bool).with_default(json!(true)),
                    ],
                ),
                Shape::Json,
            )
            .summary("Synthesise the pointer, scroll and focus samples the collector exports"),
        )
        .op(
            OpSpec::new(
                "deobfuscate",
                Shape::object(
                    "DeobfuscateInput",
                    [
                        field("data", Shape::Str)
                            .summary("The data-obfuscated attribute from the widget"),
                        field("max_counter", Shape::optional(Shape::Int)),
                    ],
                ),
                Shape::object(
                    "Deobfuscated",
                    [
                        field("text", Shape::Str),
                        field("counter", Shape::Int),
                        field("took_ms", Shape::Float),
                    ],
                ),
            )
            .summary("Solve and decrypt text hidden by the obfuscation plugin")
            .deadline_ms(120_000),
        )
        .op(
            OpSpec::new(
                "server_signature",
                Shape::object(
                    "ServerSignatureInput",
                    [
                        field("payload", Shape::Str),
                        field("secret", Shape::optional(Shape::Str)),
                        field("fields", Shape::optional(Shape::map(Shape::Json)))
                            .summary("Form values, checked against fieldsHash when present"),
                    ],
                ),
                Shape::object(
                    "ServerSignature",
                    [
                        field("verified", Shape::Bool),
                        field("expired", Shape::Bool),
                        field("invalid_signature", Shape::Bool),
                        field("invalid_solution", Shape::Bool),
                        field("fields_valid", Shape::optional(Shape::Bool)),
                        field("verification_data", Shape::Json),
                    ],
                ),
            )
            .summary("Parse and check a Sentinel server signature payload"),
        )
        .op(
            OpSpec::new(
                "submit",
                Shape::object(
                    "SubmitInput",
                    [
                        field("url", Shape::optional(Shape::Str))
                            .summary("Overrides verify_url from the session config"),
                        field("payload", Shape::Str),
                        field("code", Shape::optional(Shape::Str)),
                        field("fields", Shape::optional(Shape::map(Shape::Json))),
                    ],
                ),
                Shape::object(
                    "Submitted",
                    [
                        field("status", Shape::Int),
                        field("ok", Shape::Bool),
                        field("body", Shape::Json),
                    ],
                ),
            )
            .summary("Post a payload to a server verification endpoint")
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
}

fn challenge_shape() -> Shape {
    Shape::object(
        "Challenge",
        [
            field("challenge", Shape::Json).summary("The challenge exactly as the server sent it"),
            field("format", Shape::Int).summary("1 for the legacy format, 3 otherwise"),
            field("algorithm", Shape::Str),
            field("cost", Shape::Int),
            field("key_length", Shape::Int),
            field("expires_at", Shape::optional(Shape::Int)),
        ],
    )
}

fn config_shape() -> Shape {
    Shape::object(
        "AltchaConfig",
        [
            field("challenge_url", Shape::optional(Shape::Str))
                .summary("Endpoint the widget fetches its challenge from"),
            field("verify_url", Shape::optional(Shape::Str))
                .summary("Endpoint used by submit"),
            field("hmac_secret", Shape::optional(Shape::Str))
                .summary("Server secret, only needed by verify and create_challenge"),
            field("workers", Shape::Int)
                .summary("Solver threads, 0 picks one per core")
                .with_default(json!(0)),
            field("max_counter", Shape::Int)
                .summary("Highest counter the solver will try before giving up")
                .with_default(json!(5_000_000)),
            field("his", Shape::Bool)
                .summary("Answer an interaction signature request when the server asks for one")
                .with_default(json!(true)),
            field("proxy", Shape::optional(Shape::Str)),
            field("fingerprint", Shape::optional(Shape::Str))
                .summary("Client to emulate as profile[:platform], for example chrome_141:windows"),
            field("user_agent", Shape::optional(Shape::Str))
                .summary("User agent to send, which also picks the matching transport fingerprint"),
            field("clock_ms", Shape::optional(Shape::Int))
                .summary("Freeze the clock, which fixes the expiry checks"),
            field("seed", Shape::optional(Shape::Int))
                .summary("Seed the random source, which fixes the his samples"),
            field("timeout_ms", Shape::Int).with_default(json!(30_000)),
        ],
    )
}

#[derive(Debug, Clone, Deserialize)]
struct Config {
    #[serde(default)]
    challenge_url: Option<String>,
    #[serde(default)]
    verify_url: Option<String>,
    #[serde(default)]
    hmac_secret: Option<String>,
    #[serde(default)]
    workers: usize,
    #[serde(default = "default_max_counter")]
    max_counter: u64,
    #[serde(default = "default_true")]
    his: bool,
    #[serde(default)]
    proxy: Option<String>,
    #[serde(default)]
    fingerprint: Option<String>,
    #[serde(default)]
    user_agent: Option<String>,
    #[serde(default)]
    clock_ms: Option<u64>,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default = "default_timeout")]
    timeout_ms: u64,
}

fn default_max_counter() -> u64 {
    5_000_000
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

    let mut ctx = ctx;
    if let Some(ms) = config.clock_ms {
        ctx = ctx.with_clock(Clock::Fixed(ms));
    }
    if let Some(seed) = config.seed {
        ctx = ctx.with_seed(seed);
    }

    let workers = resolve_workers(config.workers);
    ctx.fact("challenge_url", json!(config.challenge_url));
    ctx.fact("workers", json!(workers));
    ctx.fact("max_counter", json!(config.max_counter));

    Ok(Box::new(Altcha { ctx, config, workers, solved: 0, last: Value::Null }))
}

struct Altcha {
    ctx: Ctx,
    config: Config,
    workers: usize,
    solved: u64,
    last: Value,
}

impl Altcha {
    fn http(&self) -> ClientResult<Http> {
        let mut options = HttpOptions::with_proxy(self.config.proxy.as_deref());
        options.fingerprint = self.config.fingerprint.clone();
        options.user_agent = self.config.user_agent.clone();
        options.timeout_secs = Some(self.config.timeout_ms.div_ceil(1000).max(1));
        self.ctx.http_with(options)
    }

    fn fetch_challenge(&mut self, url: &str, his: bool, call: &Call) -> ClientResult<Value> {
        let http = self.http()?;
        let response = http.fetch(FetchRequest::get(url))?;
        reject_status(url, response.status)?;

        let json: Value = serde_json::from_slice(&response.body).map_err(|error| {
            ClientError::drift(format!("{url} did not answer with json: {error}"))
        })?;

        let request = json.get("his").and_then(|value| value.get("url"));
        let Some(his_url) = request.and_then(Value::as_str) else {
            return Ok(json);
        };

        if !his {
            return Err(ClientError::blocked(
                "the server asked for an interaction signature and his is off",
            )
            .with_detail(json!({ "his_url": his_url })));
        }

        let his_url = absolute(url, his_url)?;
        call.debug("his", json!({ "url": his_url }));

        let samples = self.synthesize_his(&Map::new());
        let body = serde_json::to_vec(&json!({ "his": samples })).unwrap_or_default();
        let response = http.fetch(
            FetchRequest::post(his_url.clone(), body).header("content-type", "application/json"),
        )?;
        reject_status(&his_url, response.status)?;

        serde_json::from_slice(&response.body).map_err(|error| {
            ClientError::drift(format!("{his_url} did not answer with json: {error}"))
        })
    }

    fn synthesize_his(&self, params: &Map<String, Value>) -> Value {
        let width = number(params, "width").unwrap_or(1280.0);
        let height = number(params, "height").unwrap_or(800.0);

        let options = his::Options {
            width,
            height,
            target_x: number(params, "target_x").unwrap_or(width * 0.3),
            target_y: number(params, "target_y").unwrap_or(height * 0.62),
            duration_ms: number(params, "duration_ms").unwrap_or(1400.0),
            start_ms: number(params, "start_ms").unwrap_or(900.0),
            touch: params.get("touch").and_then(Value::as_bool).unwrap_or(false),
            scroll: params.get("scroll").and_then(Value::as_bool).unwrap_or(true),
            now_ms: self.ctx.now_ms(),
        };

        his::synthesize(&options, &mut his::Random::new(self.ctx.random_u64()))
    }

    fn solve_parameters(
        &self,
        parameters: &Parameters,
        mode: CounterMode,
        max_counter: u64,
        workers: usize,
        call: &Call,
    ) -> ClientResult<pow::Solution> {
        let stop = || call.check().is_err();
        let outcome = pow::solve(parameters, mode, 0, max_counter, workers, &stop)
            .map_err(|error| ClientError::bad_input(error))?;

        call.check()?;

        outcome.ok_or_else(|| {
            ClientError::internal(format!(
                "no counter below {max_counter} produced the required key prefix"
            ))
            .with_detail(json!({
                "algorithm": parameters.algorithm,
                "cost": parameters.cost,
                "key_prefix": parameters.key_prefix,
                "max_counter": max_counter,
            }))
        })
    }

    fn verify_payload(&self, params: &Value) -> ClientResult<Value> {
        let secret = params
            .get("secret")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| self.config.hmac_secret.clone())
            .ok_or_else(|| ClientError::bad_input("a hmac secret is required to verify"))?;

        let (source, solution) = match params.get("payload").and_then(Value::as_str) {
            Some(encoded) => {
                let decoded = decode_bytes(encoded)
                    .map_err(|error| ClientError::bad_input(format!("payload: {error}")))?;
                let value: Value = serde_json::from_slice(&decoded).map_err(|error| {
                    ClientError::bad_input(format!("payload is not json: {error}"))
                })?;

                match value.get("challenge") {
                    Some(inner) if inner.is_object() => {
                        (inner.clone(), value.get("solution").cloned().unwrap_or(Value::Null))
                    }
                    _ => (value.clone(), Value::Null),
                }
            }
            None => (
                params
                    .get("challenge")
                    .cloned()
                    .ok_or_else(|| ClientError::bad_input("payload or challenge is required"))?,
                params.get("solution").cloned().unwrap_or(Value::Null),
            ),
        };

        let parsed = challenge::parse(&source)
            .map_err(|error| ClientError::bad_input(format!("challenge: {error}")))?;

        let counter = match parsed.version {
            Version::V1 => source.get("number").and_then(Value::as_u64),
            Version::V3 => solution.get("counter").and_then(Value::as_u64),
        }
        .ok_or_else(|| ClientError::bad_input("the payload carries no counter"))?;

        let expired = parsed
            .expires_at
            .is_some_and(|expires| expires < (self.ctx.now_ms() / 1_000) as i64);

        let signature = parsed.signature.clone().unwrap_or_default();
        let expected = match parsed.version {
            Version::V1 => challenge::hmac_hex(
                &parsed.parameters.algorithm,
                parsed.parameters.key_prefix.as_bytes(),
                &secret,
            ),
            Version::V3 => challenge::hmac_hex(
                "SHA-256",
                challenge::canonical_json(&parsed.raw_parameters).as_bytes(),
                &secret,
            ),
        }
        .map_err(ClientError::internal)?;

        let invalid_signature = !challenge::constant_time_equal(&signature, &expected);

        let derived = pow::derive_key(
            &parsed.parameters,
            &pow::password(&parsed.parameters.nonce, counter, parsed.counter_mode()),
        )
        .map_err(ClientError::bad_input)?;
        let derived = hex::encode(derived);

        let claimed = match parsed.version {
            Version::V1 => parsed.parameters.key_prefix.clone(),
            Version::V3 => solution
                .get("derivedKey")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        };

        let invalid_solution = !challenge::constant_time_equal(&claimed, &derived);

        Ok(json!({
            "verified": !expired && !invalid_signature && !invalid_solution,
            "expired": expired,
            "invalid_signature": invalid_signature,
            "invalid_solution": invalid_solution,
            "format": format_of(parsed.version),
        }))
    }

    fn create_challenge(&self, params: &Value, call: &Call) -> ClientResult<Value> {
        let entries = params.as_object().cloned().unwrap_or_default();
        let algorithm = text(&entries, "algorithm").unwrap_or_else(|| pow::SHA_256.to_string());
        let format = number(&entries, "format").unwrap_or(3.0) as u8;
        let cost = number(&entries, "cost").unwrap_or(100_000.0) as u32;
        let key_length = number(&entries, "key_length").unwrap_or(32.0) as usize;
        let secret = text(&entries, "secret").or_else(|| self.config.hmac_secret.clone());

        let nonce = text(&entries, "nonce").unwrap_or_else(|| self.ctx.random_hex(16));
        let salt = text(&entries, "salt").unwrap_or_else(|| self.ctx.random_hex(16));
        let expires_at = number(&entries, "expires_in_s")
            .map(|seconds| (self.ctx.now_ms() / 1_000) as f64 + seconds);
        let counter = number(&entries, "counter").unwrap_or(23.0) as u64;

        if format == 1 {
            let salt = match expires_at {
                Some(expires) => format!("{salt}?expires={}", expires as i64),
                None => salt,
            };
            let mut parameters = Parameters {
                algorithm: algorithm.clone(),
                nonce: salt.as_bytes().to_vec(),
                salt: Vec::new(),
                cost: 1,
                key_length: match algorithm.as_str() {
                    "SHA-512" => 64,
                    "SHA-384" => 48,
                    _ => 32,
                },
                key_prefix: String::new(),
                memory_cost: None,
                parallelism: None,
            };

            let derived = pow::derive_key(
                &parameters,
                &pow::password(&parameters.nonce, counter, CounterMode::Text),
            )
            .map_err(ClientError::bad_input)?;
            parameters.key_prefix = hex::encode(derived);

            let signature = match &secret {
                Some(secret) => Some(
                    challenge::hmac_hex(&algorithm, parameters.key_prefix.as_bytes(), secret)
                        .map_err(ClientError::internal)?,
                ),
                None => None,
            };

            let challenge = json!({
                "algorithm": algorithm,
                "challenge": parameters.key_prefix,
                "salt": salt,
                "signature": signature,
            });

            return Ok(json!({
                "challenge": challenge,
                "format": 1,
                "algorithm": algorithm,
                "cost": 1,
                "key_length": parameters.key_length,
                "expires_at": expires_at.map(|value| value as i64),
            }));
        }

        let mut parameters = Parameters {
            algorithm: algorithm.clone(),
            nonce: hex::decode(&nonce)
                .map_err(|error| ClientError::bad_input(format!("nonce is not hex: {error}")))?,
            salt: hex::decode(&salt)
                .map_err(|error| ClientError::bad_input(format!("salt is not hex: {error}")))?,
            cost,
            key_length,
            key_prefix: String::new(),
            memory_cost: number(&entries, "memory_cost").map(|value| value as u32),
            parallelism: number(&entries, "parallelism").map(|value| value as u32),
        };

        call.check()?;
        let derived = pow::derive_key(
            &parameters,
            &pow::password(&parameters.nonce, counter, CounterMode::Uint32),
        )
        .map_err(ClientError::bad_input)?;

        let prefix_length = number(&entries, "key_prefix_length")
            .map(|value| value as usize)
            .unwrap_or(key_length / 2)
            .min(derived.len());
        parameters.key_prefix = hex::encode(&derived[..prefix_length]);

        let mut raw = Map::new();
        raw.insert("algorithm".to_string(), json!(algorithm));
        raw.insert("cost".to_string(), json!(cost));
        if let Some(expires) = expires_at {
            raw.insert("expiresAt".to_string(), json!(expires as i64));
        }
        raw.insert("keyLength".to_string(), json!(key_length));
        raw.insert("keyPrefix".to_string(), json!(parameters.key_prefix));
        if let Some(memory_cost) = parameters.memory_cost {
            raw.insert("memoryCost".to_string(), json!(memory_cost));
        }
        raw.insert("nonce".to_string(), json!(nonce));
        if let Some(parallelism) = parameters.parallelism {
            raw.insert("parallelism".to_string(), json!(parallelism));
        }
        raw.insert("salt".to_string(), json!(salt));

        let raw = Value::Object(raw);
        let signature = match &secret {
            Some(secret) => Some(
                challenge::hmac_hex(
                    "SHA-256",
                    challenge::canonical_json(&raw).as_bytes(),
                    secret,
                )
                .map_err(ClientError::internal)?,
            ),
            None => None,
        };

        Ok(json!({
            "challenge": { "parameters": raw, "signature": signature },
            "format": 3,
            "algorithm": algorithm,
            "cost": cost,
            "key_length": key_length,
            "expires_at": expires_at.map(|value| value as i64),
        }))
    }
}

impl Client for Altcha {
    fn call(&mut self, op: &str, params: Value, call: &Call) -> ClientResult<Value> {
        call.check()?;
        let started = Instant::now();

        let outcome = match op {
            "info" => Ok(json!({
                "target": ID,
                "version": env!("CARGO_PKG_VERSION"),
                "algorithms": pow::ALGORITHMS,
                "workers": self.workers,
            })),

            "challenge" => {
                let url = params
                    .get("url")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| self.config.challenge_url.clone())
                    .ok_or_else(|| ClientError::bad_input("url or challenge_url is required"))?;
                let his = params
                    .get("his")
                    .and_then(Value::as_bool)
                    .unwrap_or(self.config.his);

                let source = self.fetch_challenge(&url, his, call)?;
                describe_challenge(&source)
            }

            "solve" => {
                let his = params
                    .get("his")
                    .and_then(Value::as_bool)
                    .unwrap_or(self.config.his);
                let max_counter = params
                    .get("max_counter")
                    .and_then(Value::as_u64)
                    .unwrap_or(self.config.max_counter);
                let workers = params
                    .get("workers")
                    .and_then(Value::as_u64)
                    .map(|value| resolve_workers(value as usize))
                    .unwrap_or(self.workers);

                let source = match params.get("challenge") {
                    Some(value) if !value.is_null() => value.clone(),
                    _ => {
                        call.progress(1, 3, "fetching challenge");
                        let url = params
                            .get("url")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .or_else(|| self.config.challenge_url.clone())
                            .ok_or_else(|| {
                                ClientError::bad_input(
                                    "solve needs a challenge, a url or challenge_url in the config",
                                )
                            })?;
                        self.fetch_challenge(&url, his, call)?
                    }
                };

                let parsed = challenge::parse(&source)
                    .map_err(|error| ClientError::drift(format!("challenge: {error}")))?;
                call.debug(
                    "challenge",
                    json!({
                        "algorithm": parsed.parameters.algorithm,
                        "cost": parsed.parameters.cost,
                        "format": format_of(parsed.version),
                        "prefix_bytes": parsed.parameters.key_prefix.len() / 2,
                    }),
                );

                call.progress(2, 3, "solving");
                let solution = self.solve_parameters(
                    &parsed.parameters,
                    parsed.counter_mode(),
                    max_counter,
                    workers,
                    call,
                )?;

                let took_ms = round_tenth(started.elapsed().as_secs_f64() * 1_000.0);
                let payload = parsed.payload(solution.counter, &solution.derived_key, took_ms);
                let encoded = encode_bytes(serde_json::to_string(&payload).unwrap_or_default().as_bytes());

                call.progress(3, 3, "done");
                self.solved += 1;
                self.ctx.count("altcha.solve");
                self.ctx.metric("altcha.attempts", solution.attempts as f64);

                let result = json!({
                    "payload": encoded,
                    "field": FIELD_NAME,
                    "counter": solution.counter,
                    "derived_key": hex::encode(&solution.derived_key),
                    "algorithm": parsed.parameters.algorithm,
                    "format": format_of(parsed.version),
                    "attempts": solution.attempts,
                    "took_ms": took_ms,
                });
                self.last = json!({
                    "counter": solution.counter,
                    "attempts": solution.attempts,
                    "algorithm": parsed.parameters.algorithm,
                    "cost": parsed.parameters.cost,
                    "took_ms": took_ms,
                });

                Ok(result)
            }

            "derive_key" => {
                let entries = params.as_object().cloned().unwrap_or_default();
                let mode = match text(&entries, "counter_mode").as_deref() {
                    Some("string") => CounterMode::Text,
                    _ => CounterMode::Uint32,
                };
                let nonce = text(&entries, "nonce").unwrap_or_default();
                let nonce = match mode {
                    CounterMode::Text => nonce.as_bytes().to_vec(),
                    CounterMode::Uint32 => hex::decode(&nonce).map_err(|error| {
                        ClientError::bad_input(format!("nonce is not hex: {error}"))
                    })?,
                };

                let parameters = Parameters {
                    algorithm: text(&entries, "algorithm")
                        .ok_or_else(|| ClientError::bad_input("algorithm is required"))?,
                    nonce,
                    salt: hex::decode(text(&entries, "salt").unwrap_or_default()).map_err(
                        |error| ClientError::bad_input(format!("salt is not hex: {error}")),
                    )?,
                    cost: number(&entries, "cost").unwrap_or(1.0) as u32,
                    key_length: number(&entries, "key_length").unwrap_or(32.0) as usize,
                    key_prefix: String::new(),
                    memory_cost: number(&entries, "memory_cost").map(|value| value as u32),
                    parallelism: number(&entries, "parallelism").map(|value| value as u32),
                };

                let counter = number(&entries, "counter").unwrap_or(0.0) as u64;
                let password = pow::password(&parameters.nonce, counter, mode);
                let key = pow::derive_key(&parameters, &password).map_err(ClientError::bad_input)?;

                Ok(json!({ "key": hex::encode(key), "password": hex::encode(password) }))
            }

            "verify" => self.verify_payload(&params),

            "create_challenge" => self.create_challenge(&params, call),

            "his" => Ok(self.synthesize_his(&params.as_object().cloned().unwrap_or_default())),

            "deobfuscate" => {
                let data = params
                    .get("data")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ClientError::bad_input("data is required"))?;
                let max_counter = params
                    .get("max_counter")
                    .and_then(Value::as_u64)
                    .unwrap_or(self.config.max_counter);

                let parsed = obfuscation::parse(data).map_err(ClientError::bad_input)?;
                let solution = self.solve_parameters(
                    &parsed.parameters,
                    obfuscation::COUNTER_MODE,
                    max_counter,
                    self.workers,
                    call,
                )?;

                let text = obfuscation::decrypt(&solution.derived_key, &parsed.iv, &parsed.data)
                    .map_err(ClientError::drift)?;

                Ok(json!({
                    "text": text,
                    "counter": solution.counter,
                    "took_ms": round_tenth(started.elapsed().as_secs_f64() * 1_000.0),
                }))
            }

            "server_signature" => {
                let encoded = params
                    .get("payload")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ClientError::bad_input("payload is required"))?;
                let secret = params
                    .get("secret")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| self.config.hmac_secret.clone())
                    .ok_or_else(|| ClientError::bad_input("a hmac secret is required"))?;

                let decoded = decode_bytes(encoded)
                    .map_err(|error| ClientError::bad_input(format!("payload: {error}")))?;
                let payload: Value = serde_json::from_slice(&decoded).map_err(|error| {
                    ClientError::bad_input(format!("payload is not json: {error}"))
                })?;

                let form = params.get("fields").and_then(Value::as_object);
                let checked = signature::verify(
                    &payload,
                    &secret,
                    (self.ctx.now_ms() / 1_000) as i64,
                    form,
                )
                .map_err(ClientError::bad_input)?;

                Ok(json!({
                    "verified": checked.get("verified").cloned().unwrap_or(json!(false)),
                    "expired": checked.get("expired").cloned().unwrap_or(json!(false)),
                    "invalid_signature": checked
                        .get("invalidSignature")
                        .cloned()
                        .unwrap_or(json!(true)),
                    "invalid_solution": checked
                        .get("invalidSolution")
                        .cloned()
                        .unwrap_or(json!(true)),
                    "fields_valid": checked.get("fieldsValid").cloned().unwrap_or(Value::Null),
                    "verification_data": checked
                        .get("verificationData")
                        .cloned()
                        .unwrap_or(Value::Null),
                }))
            }

            "submit" => {
                let url = params
                    .get("url")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| self.config.verify_url.clone())
                    .ok_or_else(|| ClientError::bad_input("url or verify_url is required"))?;
                let payload = params
                    .get("payload")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ClientError::bad_input("payload is required"))?;

                let mut body = Map::new();
                body.insert("payload".to_string(), json!(payload));
                if let Some(code) = params.get("code").and_then(Value::as_str) {
                    body.insert("code".to_string(), json!(code));
                }
                if let Some(fields) = params.get("fields").and_then(Value::as_object) {
                    for (name, value) in fields {
                        body.insert(name.clone(), value.clone());
                    }
                }

                let http = self.http()?;
                let request = FetchRequest::post(
                    url.clone(),
                    serde_json::to_vec(&Value::Object(body)).unwrap_or_default(),
                )
                .header("content-type", "application/json");

                let response = http.fetch(request)?;
                reject_status(&url, response.status)?;

                let parsed = serde_json::from_slice(&response.body)
                    .unwrap_or_else(|_| json!(response.text()));

                Ok(json!({
                    "status": response.status,
                    "ok": (200..300).contains(&response.status),
                    "body": parsed,
                }))
            }

            other => Err(ClientError::unsupported(format!("{ID} has no op {other}"))),
        };

        self.ctx
            .metric(&format!("{ID}.{op}.ms"), started.elapsed().as_millis() as f64);

        outcome.map_err(|error| error.with_op(op).with_target(ID))
    }

    fn health(&mut self) -> ClientResult<Value> {
        Ok(json!({
            "ok": true,
            "target": ID,
            "detail": { "solved": self.solved, "workers": self.workers },
        }))
    }

    fn diagnostics(&mut self) -> Value {
        json!({
            "solved": self.solved,
            "workers": self.workers,
            "last_solve": self.last,
            "config": {
                "challenge_url": self.config.challenge_url,
                "verify_url": self.config.verify_url,
                "his": self.config.his,
                "max_counter": self.config.max_counter,
                "proxy_set": self.config.proxy.is_some(),
                "secret_set": self.config.hmac_secret.is_some(),
                "timeout_ms": self.config.timeout_ms,
            },
        })
    }
}

fn describe_challenge(source: &Value) -> ClientResult<Value> {
    let parsed = challenge::parse(source)
        .map_err(|error| ClientError::drift(format!("challenge: {error}")))?;

    Ok(json!({
        "challenge": source,
        "format": format_of(parsed.version),
        "algorithm": parsed.parameters.algorithm,
        "cost": parsed.parameters.cost,
        "key_length": parsed.parameters.key_length,
        "expires_at": parsed.expires_at,
    }))
}

fn format_of(version: Version) -> u8 {
    match version {
        Version::V1 => 1,
        Version::V3 => 3,
    }
}

fn resolve_workers(requested: usize) -> usize {
    if requested > 0 {
        return requested.min(64);
    }
    std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(4)
}

fn reject_status(url: &str, status: u16) -> ClientResult<()> {
    if status == 403 || status == 429 {
        return Err(ClientError::blocked(format!("{url} answered {status}"))
            .with_detail(json!({ "status": status })));
    }
    if !(200..300).contains(&status) {
        return Err(ClientError::resource(format!("{url} answered {status}"))
            .with_detail(json!({ "status": status })));
    }
    Ok(())
}

fn absolute(base: &str, target: &str) -> ClientResult<String> {
    if target.starts_with("http://") || target.starts_with("https://") {
        return Ok(target.to_string());
    }
    url::Url::parse(base)
        .and_then(|base| base.join(target))
        .map(|joined| joined.to_string())
        .map_err(|error| ClientError::bad_input(format!("{target} is not a usable url: {error}")))
}

fn round_tenth(value: f64) -> f64 {
    (value * 10.0).floor() / 10.0
}

fn text(entries: &Map<String, Value>, key: &str) -> Option<String> {
    entries.get(key).and_then(Value::as_str).map(str::to_string)
}

fn number(entries: &Map<String, Value>, key: &str) -> Option<f64> {
    entries.get(key).and_then(Value::as_f64)
}
