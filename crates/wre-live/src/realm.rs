use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Once};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use wre_core::error::{Error, Result};

use crate::prelude;

static PLATFORM: Once = Once::new();

pub fn initialize() {
    PLATFORM.call_once(|| {
        let platform = v8::new_default_platform(0, false).make_shared();
        v8::V8::initialize_platform(platform);
        v8::V8::initialize();
    });
}

pub type HostFn = Box<dyn Fn(&[Value]) -> Result<Value> + Send + Sync>;

struct HostEntry {
    name: String,
    handler: HostFn,
}

#[derive(Debug, Clone)]
pub struct RealmOptions {
    pub timeout: Duration,
    pub clock_ms: Option<f64>,
    pub random_seed: Option<u64>,
    pub timers: bool,
    pub codecs: bool,
    pub heap_limit_mb: Option<usize>,
}

impl Default for RealmOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            clock_ms: None,
            random_seed: None,
            timers: true,
            codecs: true,
            heap_limit_mb: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Records {
    #[serde(default)]
    pub console: Vec<ConsoleLine>,
    #[serde(default)]
    pub access: Vec<AccessRecord>,
    #[serde(default)]
    pub errors: Vec<ErrorRecord>,
    #[serde(default)]
    pub calls: Vec<CallRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleLine {
    pub level: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRecord {
    pub on: String,
    pub kind: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorRecord {
    pub where_: Option<String>,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallRecord {
    pub fn_: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub threw: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FunctionHandle {
    pub name: String,
    slot: usize,
}

impl FunctionHandle {
    pub fn slot(&self) -> usize {
        self.slot
    }
}

pub struct Realm {
    isolate: v8::OwnedIsolate,
    context: v8::Global<v8::Context>,
    functions: Vec<v8::Global<v8::Function>>,
    hosts: Vec<Arc<HostEntry>>,
    options: RealmOptions,
}

impl Realm {
    pub fn new(options: RealmOptions) -> Result<Self> {
        initialize();

        let mut params = v8::CreateParams::default();
        if let Some(megabytes) = options.heap_limit_mb {
            params = params.heap_limits(0, megabytes * 1024 * 1024);
        }

        let mut isolate = v8::Isolate::new(params);

        let context = {
            v8::scope!(let handle_scope, &mut isolate);
            let context = v8::Context::new(handle_scope, Default::default());
            v8::Global::new(handle_scope, context)
        };

        let mut realm = Self {
            isolate,
            context,
            functions: Vec::new(),
            hosts: Vec::new(),
            options,
        };

        realm.run_prelude()?;
        Ok(realm)
    }

    pub fn plain() -> Result<Self> {
        Self::new(RealmOptions::default())
    }

    fn run_prelude(&mut self) -> Result<()> {
        self.eval_unit(prelude::RUNTIME, "wre:runtime")?;

        if let Some(epoch) = self.options.clock_ms {
            let source = prelude::clock(epoch);
            self.eval_unit(&source, "wre:clock")?;
        }

        if let Some(seed) = self.options.random_seed {
            let source = prelude::random(seed);
            self.eval_unit(&source, "wre:random")?;
        }

        if self.options.timers {
            self.eval_unit(prelude::TIMERS, "wre:timers")?;
        }

        if self.options.codecs {
            self.eval_unit(prelude::CODECS, "wre:codecs")?;
        }

        self.eval_unit(prelude::ACCESS_TRAP, "wre:trap")?;
        Ok(())
    }

    pub fn options(&self) -> &RealmOptions {
        &self.options
    }

    fn guard(&mut self) -> Guard {
        Guard::start(self.isolate.thread_safe_handle(), self.options.timeout)
    }

    fn enter(&self) -> Entered {
        let pointer: *const v8::Isolate = &*self.isolate;
        unsafe { (*pointer).enter() };
        Entered(pointer)
    }

    pub fn eval_unit(&mut self, source: &str, name: &str) -> Result<()> {
        self.eval_value(source, name).map(|_| ())
    }

    pub fn eval(&mut self, source: &str, name: &str) -> Result<Value> {
        self.eval_value(source, name)
    }

    pub fn eval_json(&mut self, expression: &str) -> Result<Value> {
        let wrapped = format!("(function () {{ return ({expression}); }})()");
        self.eval_value(&wrapped, "wre:eval")
    }

    fn eval_value(&mut self, source: &str, name: &str) -> Result<Value> {
        let _entered = self.enter();
        let guard = self.guard();
        let context = self.context.clone();

        let outcome = {
            v8::scope_with_context!(let scope, &mut self.isolate, &context);
            v8::tc_scope!(let tc, scope);

            let Some(code) = v8::String::new(tc, source) else {
                return Err(Error::msg(format!("{name}: source too large for v8")));
            };

            let origin_name = v8::String::new(tc, name).unwrap_or_else(|| {
                v8::String::new(tc, "wre:anonymous").expect("short string")
            });

            let origin = v8::ScriptOrigin::new(
                tc,
                origin_name.into(),
                0,
                0,
                false,
                0,
                None,
                false,
                false,
                false,
                None,
            );

            let Some(script) = v8::Script::compile(tc, code, Some(&origin)) else {
                return Err(Error::msg(format!(
                    "{name}: compile failed: {}",
                    describe_exception(tc)
                )));
            };

            match script.run(tc) {
                Some(value) => to_json(tc, value),
                None => {
                    if tc.has_terminated() {
                        return Err(Error::msg(format!(
                            "{name}: execution ran past the {:?} budget",
                            self.options.timeout
                        )));
                    }
                    return Err(Error::msg(format!(
                        "{name}: {}",
                        describe_exception(tc)
                    )));
                }
            }
        };

        guard.finish();
        Ok(outcome)
    }

    pub fn capture(&mut self, name: &str, expression: &str) -> Result<FunctionHandle> {
        let _entered = self.enter();
        let context = self.context.clone();
        let guard = self.guard();

        let stored = {
            v8::scope_with_context!(let scope, &mut self.isolate, &context);
            v8::tc_scope!(let tc, scope);

            let wrapped = format!("({expression})");
            let Some(code) = v8::String::new(tc, &wrapped) else {
                return Err(Error::msg(format!("{name}: expression too large")));
            };

            let Some(script) = v8::Script::compile(tc, code, None) else {
                return Err(Error::msg(format!(
                    "{name}: expression did not compile: {}",
                    describe_exception(tc)
                )));
            };

            let Some(value) = script.run(tc) else {
                return Err(Error::msg(format!(
                    "{name}: expression threw: {}",
                    describe_exception(tc)
                )));
            };

            let Ok(function) = v8::Local::<v8::Function>::try_from(value) else {
                return Err(Error::msg(format!("{name}: {expression} is not a function")));
            };

            v8::Global::new(tc, function)
        };

        guard.finish();
        self.functions.push(stored);

        Ok(FunctionHandle {
            name: name.to_string(),
            slot: self.functions.len() - 1,
        })
    }

    pub fn call(&mut self, handle: &FunctionHandle, args: &[Value]) -> Result<Value> {
        let _entered = self.enter();
        let Some(stored) = self.functions.get(handle.slot).cloned() else {
            return Err(Error::msg(format!("handle {} is not live", handle.name)));
        };

        let context = self.context.clone();
        let guard = self.guard();

        let outcome = {
            v8::scope_with_context!(let scope, &mut self.isolate, &context);
            v8::tc_scope!(let tc, scope);

            let function = v8::Local::new(tc, &stored);
            let mut locals = Vec::with_capacity(args.len());

            for argument in args {
                locals.push(from_json(tc, argument)?);
            }

            let receiver = v8::undefined(tc).into();

            match function.call(tc, receiver, &locals) {
                Some(value) => to_json(tc, value),
                None => {
                    if tc.has_terminated() {
                        return Err(Error::msg(format!(
                            "{}: execution ran past the {:?} budget",
                            handle.name, self.options.timeout
                        )));
                    }
                    return Err(Error::msg(format!(
                        "{} threw: {}",
                        handle.name,
                        describe_exception(tc)
                    )));
                }
            }
        };

        guard.finish();
        Ok(outcome)
    }

    pub fn set_global(&mut self, name: &str, value: &Value) -> Result<()> {
        let _entered = self.enter();
        let context = self.context.clone();
        v8::scope_with_context!(let scope, &mut self.isolate, &context);

        let global = scope.get_current_context().global(scope);
        let Some(key) = v8::String::new(scope, name) else {
            return Err(Error::msg(format!("global name {name} is not usable")));
        };

        let local = from_json(scope, value)?;
        global.set(scope, key.into(), local);
        Ok(())
    }

    pub fn get_global(&mut self, name: &str) -> Result<Value> {
        self.eval_json(&format!("globalThis[{}]", serde_json::to_string(name).unwrap_or_default()))
    }

    pub fn has_global(&mut self, name: &str) -> bool {
        self.eval_json(&format!(
            "typeof globalThis[{}] !== 'undefined'",
            serde_json::to_string(name).unwrap_or_default()
        ))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    }

    pub fn register_host(&mut self, name: &str, handler: HostFn) -> Result<()> {
        let _entered = self.enter();
        let entry = Arc::new(HostEntry { name: name.to_string(), handler });
        let pointer = Arc::as_ptr(&entry) as *mut std::ffi::c_void;
        self.hosts.push(Arc::clone(&entry));

        let context = self.context.clone();
        v8::scope_with_context!(let scope, &mut self.isolate, &context);

        let global = scope.get_current_context().global(scope);
        let Some(key) = v8::String::new(scope, name) else {
            return Err(Error::msg(format!("host name {name} is not usable")));
        };

        let external = v8::External::new(scope, pointer);
        let Some(function) = v8::Function::builder(host_trampoline)
            .data(external.into())
            .build(scope)
        else {
            return Err(Error::msg(format!("could not build host function {name}")));
        };

        global.set(scope, key.into(), function.into());
        Ok(())
    }

    pub fn records(&mut self) -> Result<Records> {
        let raw = self.eval_json("__wre.drain()")?;
        Ok(parse_records(&raw))
    }

    pub fn run_timers(&mut self, rounds: usize) -> Result<usize> {
        let value = self.eval_json(&format!("__wreRunTimers({rounds})"))?;
        Ok(value.as_u64().unwrap_or(0) as usize)
    }

    pub fn pending_timers(&mut self) -> Result<usize> {
        let value = self.eval_json("__wrePendingTimers()")?;
        Ok(value.as_u64().unwrap_or(0) as usize)
    }

    pub fn watch(&mut self, holder: &str, name: &str, label: &str) -> Result<bool> {
        let value = self.eval_json(&format!(
            "__wreWatch({holder}, {}, {})",
            serde_json::to_string(name).unwrap_or_default(),
            serde_json::to_string(label).unwrap_or_default()
        ))?;
        Ok(value.as_bool().unwrap_or(false))
    }

    pub fn trace(&mut self, holder: &str, name: &str, label: &str) -> Result<bool> {
        let value = self.eval_json(&format!(
            "__wreTrace({holder}, {}, {})",
            serde_json::to_string(name).unwrap_or_default(),
            serde_json::to_string(label).unwrap_or_default()
        ))?;
        Ok(value.as_bool().unwrap_or(false))
    }

    pub fn global_names(&mut self) -> Result<Vec<String>> {
        let value = self.eval_json("Object.getOwnPropertyNames(globalThis)")?;
        Ok(value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn heap_used(&mut self) -> usize {
        let _entered = self.enter();
        self.isolate.get_heap_statistics().used_heap_size()
    }
}

fn host_trampoline(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut out: v8::ReturnValue<v8::Value>,
) {
    let Ok(external) = v8::Local::<v8::External>::try_from(args.data()) else {
        return;
    };

    let entry = unsafe { &*(external.value() as *const HostEntry) };

    let mut values = Vec::with_capacity(args.length() as usize);
    for index in 0..args.length() {
        values.push(to_json(scope, args.get(index)));
    }

    match (entry.handler)(&values) {
        Ok(value) => {
            if let Ok(local) = from_json(scope, &value) {
                out.set(local);
            }
        }
        Err(error) => {
            let text = format!("{}: {error}", entry.name);
            if let Some(message) = v8::String::new(scope, &text) {
                let exception = v8::Exception::error(scope, message);
                scope.throw_exception(exception);
            }
        }
    }
}

pub fn to_json(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Value {
    if value.is_undefined() {
        return Value::Null;
    }

    if let Some(text) = v8::json::stringify(scope, value) {
        let rendered = text.to_rust_string_lossy(scope);
        if let Ok(parsed) = serde_json::from_str(&rendered) {
            return parsed;
        }
    }

    let fallback = value.to_rust_string_lossy(scope);
    Value::String(fallback)
}

pub fn from_json<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: &Value,
) -> Result<v8::Local<'s, v8::Value>> {
    let text = serde_json::to_string(value)
        .map_err(|error| Error::msg(format!("argument is not serialisable: {error}")))?;

    let Some(code) = v8::String::new(scope, &text) else {
        return Err(Error::msg("argument is too large for v8"));
    };

    v8::json::parse(scope, code)
        .ok_or_else(|| Error::msg("argument did not survive the json boundary"))
}

fn describe_exception(scope: &mut v8::PinnedRef<'_, v8::TryCatch<v8::HandleScope>>) -> String {
    let Some(exception) = scope.exception() else {
        return "no exception recorded".to_string();
    };

    let message = exception
        .to_string(scope)
        .map(|text| text.to_rust_string_lossy(scope))
        .unwrap_or_else(|| "unreadable exception".to_string());

    let Some(stack) = scope.stack_trace() else {
        return message;
    };

    let rendered = stack.to_rust_string_lossy(scope);
    let first = rendered
        .lines()
        .nth(1)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();

    if first.is_empty() { message } else { format!("{message} ({first})") }
}

struct Entered(*const v8::Isolate);

impl Drop for Entered {
    fn drop(&mut self) {
        unsafe { (*self.0).exit() };
    }
}

struct Guard {
    done: Arc<AtomicBool>,
}

impl Guard {
    fn start(handle: v8::IsolateHandle, timeout: Duration) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&done);

        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + timeout;
            while std::time::Instant::now() < deadline {
                if flag.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            if !flag.load(Ordering::Relaxed) {
                handle.terminate_execution();
            }
        });

        Self { done }
    }

    fn finish(self) {
        self.done.store(true, Ordering::Relaxed);
    }
}

fn parse_records(raw: &Value) -> Records {
    let mut records = Records::default();

    if let Some(list) = raw.get("console").and_then(Value::as_array) {
        for entry in list {
            records.console.push(ConsoleLine {
                level: entry
                    .get("level")
                    .and_then(Value::as_str)
                    .unwrap_or("log")
                    .to_string(),
                text: entry
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }

    if let Some(list) = raw.get("access").and_then(Value::as_array) {
        for entry in list {
            records.access.push(AccessRecord {
                on: entry.get("on").and_then(Value::as_str).unwrap_or_default().to_string(),
                kind: entry.get("kind").and_then(Value::as_str).unwrap_or_default().to_string(),
                key: entry.get("key").and_then(Value::as_str).unwrap_or_default().to_string(),
            });
        }
    }

    if let Some(list) = raw.get("errors").and_then(Value::as_array) {
        for entry in list {
            records.errors.push(ErrorRecord {
                where_: entry.get("where").and_then(Value::as_str).map(str::to_string),
                text: entry.get("text").and_then(Value::as_str).unwrap_or_default().to_string(),
            });
        }
    }

    if let Some(list) = raw.get("calls").and_then(Value::as_array) {
        for entry in list {
            records.calls.push(CallRecord {
                fn_: entry.get("fn").and_then(Value::as_str).map(str::to_string),
                args: entry
                    .get("args")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                result: entry.get("result").and_then(Value::as_str).map(str::to_string),
                threw: entry.get("threw").and_then(Value::as_str).map(str::to_string),
            });
        }
    }

    records
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MountReport {
    pub roles: BTreeMap<String, bool>,
    pub records: Records,
    pub bytes: usize,
    pub patched: usize,
}
