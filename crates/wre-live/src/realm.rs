use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};
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

struct ShapeEntry {
    prototype: v8::Global<v8::Object>,
    brand: Option<String>,
    promise: bool,
    singleton: bool,
    cached: Mutex<Option<v8::Global<v8::Object>>>,
}

struct HostEntry {
    name: String,
    handler: HostFn,
    brand: Option<String>,
    state: bool,
    shape: Option<ShapeEntry>,
}

#[derive(Debug, Clone)]
pub struct Shape {
    pub prototype: String,
    pub brand: Option<String>,
    pub promise: bool,
    pub singleton: bool,
}

impl Shape {
    pub fn new(prototype: &str) -> Self {
        Self {
            prototype: prototype.to_string(),
            brand: None,
            promise: false,
            singleton: false,
        }
    }

    pub fn branded(mut self, brand: &str) -> Self {
        self.brand = Some(brand.to_string());
        self
    }

    pub fn in_a_promise(mut self) -> Self {
        self.promise = true;
        self
    }

    pub fn shared(mut self) -> Self {
        self.singleton = true;
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct HostSpec<'a> {
    pub name: &'a str,
    pub display: Option<&'a str>,
    pub receiver_brand: Option<&'a str>,
    pub state: bool,
    pub arity: i32,
    pub shape: Option<Shape>,
}

impl<'a> HostSpec<'a> {
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            display: None,
            receiver_brand: None,
            state: false,
            arity: 0,
            shape: None,
        }
    }

    pub fn taking(mut self, arity: i32) -> Self {
        self.arity = arity;
        self
    }

    pub fn called(mut self, display: &'a str) -> Self {
        self.display = Some(display);
        self
    }

    pub fn on_brand(mut self, brand: &'a str) -> Self {
        self.receiver_brand = Some(brand);
        self
    }

    pub fn with_state(mut self) -> Self {
        self.state = true;
        self
    }

    pub fn building(mut self, shape: Shape) -> Self {
        self.shape = Some(shape);
        self
    }
}

pub const BRAND_KEY: &str = "wre.brand";
pub const STATE_KEY: &str = "wre.state";
pub const NATIVE_KEY: &str = "wre.native";

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

#[derive(Debug, Clone)]
pub struct Control {
    pub name: String,
    slot: usize,
}

struct SourceMask {
    original: v8::Global<v8::Function>,
}

struct Delegate {
    inner: v8::Global<v8::Function>,
}

pub struct Realm {
    isolate: v8::OwnedIsolate,
    context: v8::Global<v8::Context>,
    frames: Vec<v8::Global<v8::Context>>,
    functions: Vec<v8::Global<v8::Function>>,
    hosts: Vec<Arc<HostEntry>>,
    options: RealmOptions,
    control: Option<v8::Global<v8::Object>>,
    controls: Vec<v8::Global<v8::Object>>,
    masks: Vec<Arc<SourceMask>>,
    delegates: Vec<Arc<Delegate>>,
}

pub fn fresh_name() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let clock = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos() as u64)
        .unwrap_or(0);

    let mut state = clock
        ^ (COUNTER
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15))
        ^ 0xD1B5_4A32_D192_ED03;

    let letters = b"abcdefghijklmnopqrstuvwxyz";
    let mut out = String::with_capacity(9);

    for _ in 0..9 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.push(letters[(state % 26) as usize] as char);
    }

    out
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
            frames: Vec::new(),
            functions: Vec::new(),
            hosts: Vec::new(),
            options,
            control: None,
            controls: Vec::new(),
            masks: Vec::new(),
            delegates: Vec::new(),
        };

        realm.run_prelude()?;
        Ok(realm)
    }

    pub fn plain() -> Result<Self> {
        Self::new(RealmOptions::default())
    }

    fn run_prelude(&mut self) -> Result<()> {
        let core = prelude::core(self.options.timers);
        let control = self.eval_object(&core, "wre:core")?;
        self.control = Some(control);

        if let Some(epoch) = self.options.clock_ms {
            let source = prelude::clock(epoch);
            self.eval_unit(&source, "wre:clock")?;
        }

        if let Some(seed) = self.options.random_seed {
            let source = prelude::random(seed);
            self.eval_unit(&source, "wre:random")?;
        }

        if self.options.codecs {
            self.eval_unit(prelude::CODECS, "wre:codecs")?;
        }

        Ok(())
    }

    pub fn options(&self) -> &RealmOptions {
        &self.options
    }

    fn context_for(&self, frame: Option<usize>) -> Result<v8::Global<v8::Context>> {
        match frame {
            None => Ok(self.context.clone()),
            Some(index) => self
                .frames
                .get(index)
                .cloned()
                .ok_or_else(|| Error::msg(format!("frame {index} is not open"))),
        }
    }

    pub fn open_frame(&mut self) -> Result<usize> {
        let _entered = self.enter();
        let main = self.context.clone();

        let stored = {
            v8::scope!(let scope, &mut self.isolate);
            let context = v8::Context::new(scope, Default::default());

            let token = {
                let outer = v8::Local::new(scope, &main);
                outer.get_security_token(scope)
            };

            context.set_security_token(token);
            v8::Global::new(scope, context)
        };

        self.frames.push(stored);
        Ok(self.frames.len() - 1)
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub fn pump_microtasks(&mut self) {
        let _entered = self.enter();
        self.isolate.perform_microtask_checkpoint();
    }

    pub fn eval_unit_in(&mut self, frame: usize, source: &str, name: &str) -> Result<()> {
        self.eval_value_in(Some(frame), source, name).map(|_| ())
    }

    pub fn eval_json_in(&mut self, frame: usize, expression: &str) -> Result<Value> {
        let wrapped = format!("(function () {{ return ({expression}); }})()");
        self.eval_value_in(Some(frame), &wrapped, "wre:eval")
    }

    pub fn register_host_in(&mut self, frame: usize, name: &str, handler: HostFn) -> Result<()> {
        self.register_in(Some(frame), HostSpec::new(name), handler)
    }

    pub fn attach_in(&mut self, frame: usize, source: &str, name: &str) -> Result<Control> {
        let object = self.eval_object_in(Some(frame), source, name)?;
        self.controls.push(object);

        Ok(Control {
            name: name.to_string(),
            slot: self.controls.len() - 1,
        })
    }

    pub fn share_into(&mut self, frame: usize, expression: &str, name: &str) -> Result<()> {
        let _entered = self.enter();
        let source = self.context.clone();
        let target = self.context_for(Some(frame))?;

        let stored = {
            v8::scope_with_context!(let scope, &mut self.isolate, &source);
            v8::tc_scope!(let tc, scope);

            let Some(code) = v8::String::new(tc, expression) else {
                return Err(Error::msg(format!("expression {expression} is not usable")));
            };

            let Some(script) = v8::Script::compile(tc, code, None) else {
                return Err(Error::msg(format!(
                    "expression {expression} did not compile"
                )));
            };

            let Some(value) = script.run(tc) else {
                return Err(Error::msg(format!(
                    "expression {expression} threw: {}",
                    describe_exception(tc)
                )));
            };

            v8::Global::new(tc, value)
        };

        v8::scope_with_context!(let scope, &mut self.isolate, &target);

        let holder = scope.get_current_context().global(scope);
        let Some(key) = v8::String::new(scope, name) else {
            return Err(Error::msg(format!("global name {name} is not usable")));
        };

        let local = v8::Local::new(scope, &stored);
        holder.set(scope, key.into(), local);
        Ok(())
    }

    pub fn share_value(&mut self, frame: usize, expression: &str, name: &str) -> Result<()> {
        let _entered = self.enter();
        let source = self.context_for(Some(frame))?;
        let target = self.context.clone();

        let stored = {
            v8::scope_with_context!(let scope, &mut self.isolate, &source);
            v8::tc_scope!(let tc, scope);

            let Some(code) = v8::String::new(tc, expression) else {
                return Err(Error::msg(format!("expression {expression} is not usable")));
            };

            let Some(script) = v8::Script::compile(tc, code, None) else {
                return Err(Error::msg(format!(
                    "expression {expression} did not compile"
                )));
            };

            let Some(value) = script.run(tc) else {
                return Err(Error::msg(format!(
                    "expression {expression} threw: {}",
                    describe_exception(tc)
                )));
            };

            v8::Global::new(tc, value)
        };

        v8::scope_with_context!(let scope, &mut self.isolate, &target);

        let holder = scope.get_current_context().global(scope);
        let Some(key) = v8::String::new(scope, name) else {
            return Err(Error::msg(format!("global name {name} is not usable")));
        };

        let local = v8::Local::new(scope, &stored);
        holder.set(scope, key.into(), local);
        Ok(())
    }

    pub fn share_global(&mut self, frame: usize, into: Option<usize>, name: &str) -> Result<()> {
        let _entered = self.enter();
        let source = self.context_for(Some(frame))?;
        let target = self.context_for(into)?;

        v8::scope_with_context!(let scope, &mut self.isolate, &target);

        let shared = {
            let inner = v8::Local::new(scope, &source);
            inner.global(scope)
        };

        let holder = scope.get_current_context().global(scope);

        let Some(key) = v8::String::new(scope, name) else {
            return Err(Error::msg(format!("global name {name} is not usable")));
        };

        holder.set(scope, key.into(), shared.into());
        Ok(())
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
        self.eval_value_in(None, source, name)
    }

    fn eval_value_in(&mut self, frame: Option<usize>, source: &str, name: &str) -> Result<Value> {
        let _entered = self.enter();
        let guard = self.guard();
        let context = self.context_for(frame)?;

        let outcome = {
            v8::scope_with_context!(let scope, &mut self.isolate, &context);
            v8::tc_scope!(let tc, scope);

            let Some(code) = v8::String::new(tc, source) else {
                return Err(Error::msg(format!("{name}: source too large for v8")));
            };

            let origin_name = v8::String::new(tc, name)
                .unwrap_or_else(|| v8::String::new(tc, "wre:anonymous").expect("short string"));

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
                    return Err(Error::msg(format!("{name}: {}", describe_exception(tc))));
                }
            }
        };

        guard.finish();
        Ok(outcome)
    }

    fn eval_object(&mut self, source: &str, name: &str) -> Result<v8::Global<v8::Object>> {
        self.eval_object_in(None, source, name)
    }

    fn eval_object_in(
        &mut self,
        frame: Option<usize>,
        source: &str,
        name: &str,
    ) -> Result<v8::Global<v8::Object>> {
        let _entered = self.enter();
        let guard = self.guard();
        let context = self.context_for(frame)?;

        let stored = {
            v8::scope_with_context!(let scope, &mut self.isolate, &context);
            v8::tc_scope!(let tc, scope);

            let Some(code) = v8::String::new(tc, source) else {
                return Err(Error::msg(format!("{name}: source too large for v8")));
            };

            let Some(script) = v8::Script::compile(tc, code, None) else {
                return Err(Error::msg(format!(
                    "{name}: compile failed: {}",
                    describe_exception(tc)
                )));
            };

            let Some(value) = script.run(tc) else {
                return Err(Error::msg(format!("{name}: {}", describe_exception(tc))));
            };

            let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
                return Err(Error::msg(format!("{name}: did not return an object")));
            };

            v8::Global::new(tc, object)
        };

        guard.finish();
        Ok(stored)
    }

    fn control_invoke(
        &mut self,
        method: &str,
        holder: Option<&str>,
        args: &[Value],
    ) -> Result<Value> {
        let Some(control) = self.control.clone() else {
            return Err(Error::msg("this realm carries no instrumentation"));
        };

        self.invoke_on(control, method, holder, args)
    }

    fn invoke_on(
        &mut self,
        control: v8::Global<v8::Object>,
        method: &str,
        holder: Option<&str>,
        args: &[Value],
    ) -> Result<Value> {
        let _entered = self.enter();
        let context = self.context.clone();
        let guard = self.guard();

        let outcome = {
            v8::scope_with_context!(let scope, &mut self.isolate, &context);
            v8::tc_scope!(let tc, scope);

            let object = v8::Local::new(tc, &control);

            let Some(key) = v8::String::new(tc, method) else {
                return Err(Error::msg(format!("method name {method} is not usable")));
            };

            let Some(value) = object.get(tc, key.into()) else {
                return Err(Error::msg(format!("the instrumentation has no {method}")));
            };

            let Ok(function) = v8::Local::<v8::Function>::try_from(value) else {
                return Err(Error::msg(format!(
                    "the instrumentation's {method} is not a function"
                )));
            };

            let mut locals = Vec::with_capacity(args.len() + 1);

            if let Some(expression) = holder {
                let Some(code) = v8::String::new(tc, expression) else {
                    return Err(Error::msg(format!("expression {expression} is not usable")));
                };

                let Some(script) = v8::Script::compile(tc, code, None) else {
                    return Err(Error::msg(format!(
                        "expression {expression} did not compile"
                    )));
                };

                let Some(target) = script.run(tc) else {
                    return Err(Error::msg(format!(
                        "expression {expression} threw: {}",
                        describe_exception(tc)
                    )));
                };

                locals.push(target);
            }

            for argument in args {
                locals.push(from_json(tc, argument)?);
            }

            match function.call(tc, object.into(), &locals) {
                Some(value) => to_json(tc, value),
                None => {
                    if tc.has_terminated() {
                        return Err(Error::msg(format!(
                            "{method}: execution ran past the {:?} budget",
                            self.options.timeout
                        )));
                    }
                    return Err(Error::msg(format!(
                        "{method} threw: {}",
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
                return Err(Error::msg(format!(
                    "{name}: {expression} is not a function"
                )));
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
        self.eval_json(&format!(
            "globalThis[{}]",
            serde_json::to_string(name).unwrap_or_default()
        ))
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
        self.register(HostSpec::new(name), handler)
    }

    pub fn register_branded_host(
        &mut self,
        name: &str,
        brand: &str,
        handler: HostFn,
    ) -> Result<()> {
        self.register(HostSpec::new(name).on_brand(brand), handler)
    }

    pub fn brand_object(&mut self, expression: &str, brand: &str) -> Result<()> {
        let _entered = self.enter();
        let context = self.context.clone();
        v8::scope_with_context!(let scope, &mut self.isolate, &context);
        v8::tc_scope!(let tc, scope);

        let Some(source) = v8::String::new(tc, expression) else {
            return Err(Error::msg(format!("expression {expression} is not usable")));
        };

        let Some(script) = v8::Script::compile(tc, source, None) else {
            return Err(Error::msg(format!(
                "expression {expression} did not compile"
            )));
        };

        let Some(value) = script.run(tc) else {
            return Err(Error::msg(format!("expression {expression} did not run")));
        };

        let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
            return Err(Error::msg(format!(
                "expression {expression} is not an object"
            )));
        };

        let Some(key) = v8::String::new(tc, BRAND_KEY) else {
            return Err(Error::msg("the brand key is not usable"));
        };
        let private = v8::Private::for_api(tc, Some(key));

        let Some(marker) = v8::String::new(tc, brand) else {
            return Err(Error::msg(format!("brand {brand} is not usable")));
        };

        object.set_private(tc, private, marker.into());
        Ok(())
    }

    pub fn make_native(&mut self, holder: &str, key: &str, name: Option<&str>) -> Result<()> {
        let _entered = self.enter();
        let context = self.context.clone();

        let (entry, arity, display) = {
            v8::scope_with_context!(let scope, &mut self.isolate, &context);
            v8::tc_scope!(let tc, scope);

            let Some(source) = v8::String::new(tc, holder) else {
                return Err(Error::msg(format!("holder {holder} is not usable")));
            };

            let Some(script) = v8::Script::compile(tc, source, None) else {
                return Err(Error::msg(format!("holder {holder} did not compile")));
            };

            let Some(value) = script.run(tc) else {
                return Err(Error::msg(format!("holder {holder} did not run")));
            };

            let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
                return Err(Error::msg(format!("holder {holder} is not an object")));
            };

            let Some(member) = v8::String::new(tc, key) else {
                return Err(Error::msg(format!("member {key} is not usable")));
            };

            let Some(current) = object.get(tc, member.into()) else {
                return Err(Error::msg(format!("{holder}.{key} is not there")));
            };

            let Ok(function) = v8::Local::<v8::Function>::try_from(current) else {
                return Err(Error::msg(format!("{holder}.{key} is not a function")));
            };

            let arity = v8::String::new(tc, "length")
                .and_then(|field| function.get(tc, field.into()))
                .and_then(|value| value.to_integer(tc))
                .map(|value| value.value() as i32)
                .unwrap_or(0);

            let display = name.map(str::to_string).unwrap_or_else(|| key.to_string());

            (
                Arc::new(Delegate {
                    inner: v8::Global::new(tc, function),
                }),
                arity,
                display,
            )
        };

        self.delegates.push(Arc::clone(&entry));

        {
            v8::scope_with_context!(let scope, &mut self.isolate, &context);
            v8::tc_scope!(let tc, scope);

            let pointer = Arc::as_ptr(&entry) as *mut std::ffi::c_void;
            let external = v8::External::new(tc, pointer);

            let Some(replacement) = v8::Function::builder(delegate_trampoline)
                .data(external.into())
                .length(arity)
                .constructor_behavior(v8::ConstructorBehavior::Throw)
                .build(tc)
            else {
                return Err(Error::msg(format!("could not rebuild {holder}.{key}")));
            };

            if let Some(text) = v8::String::new(tc, &display) {
                replacement.set_name(text);
            }

            let global = tc.get_current_context().global(tc);
            let Some(slot) = v8::String::new(tc, "__wreNative") else {
                return Err(Error::msg("the handover name is not usable"));
            };

            global.set(tc, slot.into(), replacement.into());
        }

        let install = format!(
            "(function () {{ var holder = {holder}; var key = {};              var found = Object.getOwnPropertyDescriptor(holder, key);              Object.defineProperty(holder, key, {{ value: globalThis.__wreNative,              writable: found ? found.writable !== false : true,              enumerable: found ? Boolean(found.enumerable) : false,              configurable: true }}); delete globalThis.__wreNative; }})()",
            serde_json::to_string(key).unwrap_or_default()
        );

        self.eval_unit(&install, "")
    }

    pub fn register(&mut self, spec: HostSpec<'_>, handler: HostFn) -> Result<()> {
        self.register_in(None, spec, handler)
    }

    pub fn register_in(
        &mut self,
        frame: Option<usize>,
        spec: HostSpec<'_>,
        handler: HostFn,
    ) -> Result<()> {
        let _entered = self.enter();
        let context = self.context_for(frame)?;
        v8::scope_with_context!(let scope, &mut self.isolate, &context);
        v8::tc_scope!(let tc, scope);

        let shape = match &spec.shape {
            None => None,
            Some(shape) => {
                let Some(source) = v8::String::new(tc, &shape.prototype) else {
                    return Err(Error::msg(format!(
                        "prototype {} is not usable",
                        shape.prototype
                    )));
                };

                let Some(script) = v8::Script::compile(tc, source, None) else {
                    return Err(Error::msg(format!(
                        "prototype {} did not compile",
                        shape.prototype
                    )));
                };

                let Some(value) = script.run(tc) else {
                    return Err(Error::msg(format!(
                        "prototype {} did not run",
                        shape.prototype
                    )));
                };

                let Ok(prototype) = v8::Local::<v8::Object>::try_from(value) else {
                    return Err(Error::msg(format!(
                        "prototype {} is not an object",
                        shape.prototype
                    )));
                };

                Some(ShapeEntry {
                    prototype: v8::Global::new(tc, prototype),
                    brand: shape.brand.clone(),
                    promise: shape.promise,
                    singleton: shape.singleton,
                    cached: Mutex::new(None),
                })
            }
        };

        let entry = Arc::new(HostEntry {
            name: spec.name.to_string(),
            handler,
            brand: spec.receiver_brand.map(str::to_string),
            state: spec.state,
            shape,
        });

        let pointer = Arc::as_ptr(&entry) as *mut std::ffi::c_void;
        self.hosts.push(Arc::clone(&entry));

        let global = tc.get_current_context().global(tc);
        let Some(key) = v8::String::new(tc, spec.name) else {
            return Err(Error::msg(format!("host name {} is not usable", spec.name)));
        };

        let external = v8::External::new(tc, pointer);
        let Some(function) = v8::Function::builder(host_trampoline)
            .data(external.into())
            .length(spec.arity)
            .build(tc)
        else {
            return Err(Error::msg(format!(
                "could not build host function {}",
                spec.name
            )));
        };

        if let Some(display) = spec.display
            && let Some(text) = v8::String::new(tc, display)
        {
            function.set_name(text);
        }

        global.set(tc, key.into(), function.into());
        Ok(())
    }

    pub fn attach(&mut self, source: &str, name: &str) -> Result<Control> {
        let object = self.eval_object(source, name)?;
        self.controls.push(object);

        Ok(Control {
            name: name.to_string(),
            slot: self.controls.len() - 1,
        })
    }

    pub fn invoke(&mut self, control: &Control, method: &str, args: &[Value]) -> Result<Value> {
        let Some(object) = self.controls.get(control.slot).cloned() else {
            return Err(Error::msg(format!(
                "{} is not attached to this realm",
                control.name
            )));
        };

        self.invoke_on(object, method, None, args)
    }

    pub fn install_source_mask(&mut self) -> Result<()> {
        let _entered = self.enter();
        let context = self.context.clone();

        let original = {
            v8::scope_with_context!(let scope, &mut self.isolate, &context);
            v8::tc_scope!(let tc, scope);

            let Some(source) = v8::String::new(tc, "Function.prototype.toString") else {
                return Err(Error::msg("the source mask expression is not usable"));
            };

            let Some(script) = v8::Script::compile(tc, source, None) else {
                return Err(Error::msg("the source mask expression did not compile"));
            };

            let Some(value) = script.run(tc) else {
                return Err(Error::msg("Function.prototype.toString did not resolve"));
            };

            let Ok(function) = v8::Local::<v8::Function>::try_from(value) else {
                return Err(Error::msg("Function.prototype.toString is not a function"));
            };

            v8::Global::new(tc, function)
        };

        let entry = Arc::new(SourceMask { original });
        let pointer = Arc::as_ptr(&entry) as *mut std::ffi::c_void;
        self.masks.push(Arc::clone(&entry));

        v8::scope_with_context!(let scope, &mut self.isolate, &context);
        v8::tc_scope!(let tc, scope);

        let external = v8::External::new(tc, pointer);
        let Some(replacement) = v8::Function::builder(source_mask_trampoline)
            .data(external.into())
            .length(0)
            .build(tc)
        else {
            return Err(Error::msg("could not build the source mask"));
        };

        if let Some(text) = v8::String::new(tc, "toString") {
            replacement.set_name(text);
        }

        let Some(source) = v8::String::new(tc, "Function.prototype") else {
            return Err(Error::msg("Function.prototype is not reachable"));
        };

        let Some(script) = v8::Script::compile(tc, source, None) else {
            return Err(Error::msg("Function.prototype did not compile"));
        };

        let Some(value) = script.run(tc) else {
            return Err(Error::msg("Function.prototype did not resolve"));
        };

        let Ok(prototype) = v8::Local::<v8::Object>::try_from(value) else {
            return Err(Error::msg("Function.prototype is not an object"));
        };

        let Some(key) = v8::String::new(tc, "toString") else {
            return Err(Error::msg("toString is not usable as a key"));
        };

        prototype.set(tc, key.into(), replacement.into());
        Ok(())
    }

    pub fn mask_source(&mut self, expression: &str) -> Result<()> {
        let _entered = self.enter();
        let context = self.context.clone();
        v8::scope_with_context!(let scope, &mut self.isolate, &context);
        v8::tc_scope!(let tc, scope);

        let value = resolve(tc, expression)?;

        let Ok(function) = v8::Local::<v8::Function>::try_from(value) else {
            return Err(Error::msg(format!("{expression} is not a function")));
        };

        if !mark_native(tc, function.into()) {
            return Err(Error::msg(format!("{expression} could not be marked")));
        }

        Ok(())
    }

    pub fn mask_sources(&mut self, expression: &str) -> Result<usize> {
        let _entered = self.enter();
        let context = self.context.clone();
        v8::scope_with_context!(let scope, &mut self.isolate, &context);
        v8::tc_scope!(let tc, scope);

        let value = resolve(tc, expression)?;

        let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
            return Err(Error::msg(format!("{expression} is not an object")));
        };

        let Some(names) = object.get_own_property_names(tc, v8::GetPropertyNamesArgs::default())
        else {
            return Ok(0);
        };

        let mut masked = 0;

        for index in 0..names.length() {
            let Some(key) = names.get_index(tc, index) else {
                continue;
            };

            let Ok(name) = v8::Local::<v8::Name>::try_from(key) else {
                continue;
            };

            let Some(descriptor) = object.get_own_property_descriptor(tc, name) else {
                continue;
            };

            let Ok(descriptor) = v8::Local::<v8::Object>::try_from(descriptor) else {
                continue;
            };

            for slot in ["value", "get", "set"] {
                let Some(field) = v8::String::new(tc, slot) else {
                    continue;
                };

                let Some(found) = descriptor.get(tc, field.into()) else {
                    continue;
                };

                if found.is_function() && mark_native(tc, found) {
                    masked += 1;
                }
            }
        }

        Ok(masked)
    }

    pub fn records(&mut self) -> Result<Records> {
        let raw = self.control_invoke("drain", None, &[])?;
        Ok(parse_records(&raw))
    }

    pub fn run_timers(&mut self, rounds: usize) -> Result<usize> {
        let value = self.control_invoke("runTimers", None, &[Value::from(rounds)])?;
        Ok(value.as_u64().unwrap_or(0) as usize)
    }

    pub fn pending_timers(&mut self) -> Result<usize> {
        let value = self.control_invoke("pendingTimers", None, &[])?;
        Ok(value.as_u64().unwrap_or(0) as usize)
    }

    pub fn watch(&mut self, holder: &str, name: &str, label: &str) -> Result<bool> {
        let value = self.control_invoke(
            "watch",
            Some(holder),
            &[Value::from(name), Value::from(label)],
        )?;
        Ok(value.as_bool().unwrap_or(false))
    }

    pub fn trace(&mut self, holder: &str, name: &str, label: &str) -> Result<bool> {
        let value = self.control_invoke(
            "trace",
            Some(holder),
            &[Value::from(name), Value::from(label)],
        )?;
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

    pub fn heap(&mut self) -> (usize, usize) {
        let _entered = self.enter();
        let stats = self.isolate.get_heap_statistics();
        (stats.total_heap_size(), stats.used_heap_size())
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

    if let Some(brand) = &entry.brand
        && !carries_brand(scope, args.this(), brand)
    {
        if let Some(message) = v8::String::new(scope, "Illegal invocation") {
            let exception = v8::Exception::type_error(scope, message);
            scope.throw_exception(exception);
        }
        return;
    }

    let mut values = Vec::with_capacity(args.length() as usize + 1);

    if entry.state {
        match own_state(scope, args.this()) {
            Some(state) => values.push(state),
            None => {
                if let Some(message) = v8::String::new(scope, "Illegal invocation") {
                    let exception = v8::Exception::type_error(scope, message);
                    scope.throw_exception(exception);
                }
                return;
            }
        }
    }

    for index in 0..args.length() {
        values.push(to_json(scope, args.get(index)));
    }

    match (entry.handler)(&values) {
        Ok(value) => match &entry.shape {
            None => {
                if let Ok(local) = from_json(scope, &value) {
                    out.set(local);
                }
            }
            Some(shape) => {
                if let Some(local) = shaped(scope, shape, &value) {
                    out.set(local);
                }
            }
        },
        Err(error) => {
            let promised = entry.shape.as_ref().is_some_and(|shape| shape.promise);

            if promised {
                if let Some(rejected) = rejection(scope, &error.to_string()) {
                    out.set(rejected);
                    return;
                }
            }

            let text = format!("{}: {error}", entry.name);
            if let Some(message) = v8::String::new(scope, &text) {
                let exception = v8::Exception::error(scope, message);
                scope.throw_exception(exception);
            }
        }
    }
}

fn delegate_trampoline(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut out: v8::ReturnValue<v8::Value>,
) {
    let Ok(external) = v8::Local::<v8::External>::try_from(args.data()) else {
        return;
    };

    let entry = unsafe { &*(external.value() as *const Delegate) };
    let inner = v8::Local::new(scope, &entry.inner);

    let mut values = Vec::with_capacity(args.length() as usize);
    for index in 0..args.length() {
        values.push(args.get(index));
    }

    if let Some(value) = inner.call(scope, args.this().into(), &values) {
        out.set(value);
    }
}

fn source_mask_trampoline(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut out: v8::ReturnValue<v8::Value>,
) {
    let Ok(external) = v8::Local::<v8::External>::try_from(args.data()) else {
        return;
    };

    let entry = unsafe { &*(external.value() as *const SourceMask) };
    let receiver = args.this();

    if let Some(private) = private_key(scope, NATIVE_KEY)
        && let Some(found) = receiver.get_private(scope, private)
        && !found.is_undefined()
    {
        let name = v8::String::new(scope, "name")
            .and_then(|key| receiver.get(scope, key.into()))
            .map(|value| value.to_rust_string_lossy(scope))
            .unwrap_or_default();

        let text = format!("function {name}() {{ [native code] }}");

        if let Some(rendered) = v8::String::new(scope, &text) {
            out.set(rendered.into());
        }
        return;
    }

    let original = v8::Local::new(scope, &entry.original);

    if let Some(value) = original.call(scope, receiver.into(), &[]) {
        out.set(value);
    }
}

fn resolve<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    expression: &str,
) -> Result<v8::Local<'s, v8::Value>> {
    let Some(source) = v8::String::new(scope, expression) else {
        return Err(Error::msg(format!("expression {expression} is not usable")));
    };

    let Some(script) = v8::Script::compile(scope, source, None) else {
        return Err(Error::msg(format!(
            "expression {expression} did not compile"
        )));
    };

    script
        .run(scope)
        .ok_or_else(|| Error::msg(format!("expression {expression} did not run")))
}

fn mark_native(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> bool {
    let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
        return false;
    };

    let Some(private) = private_key(scope, NATIVE_KEY) else {
        return false;
    };

    let marker: v8::Local<v8::Value> = v8::Boolean::new(scope, true).into();
    object.set_private(scope, private, marker);
    true
}

fn private_key<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: &str,
) -> Option<v8::Local<'s, v8::Private>> {
    let key = v8::String::new(scope, name)?;
    Some(v8::Private::for_api(scope, Some(key)))
}

fn own_state(scope: &mut v8::PinScope, receiver: v8::Local<v8::Object>) -> Option<Value> {
    let private = private_key(scope, STATE_KEY)?;
    let found = receiver.get_private(scope, private)?;

    if found.is_undefined() {
        return None;
    }

    Some(to_json(scope, found))
}

fn shaped<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    shape: &ShapeEntry,
    value: &Value,
) -> Option<v8::Local<'s, v8::Value>> {
    if let Ok(cached) = shape.cached.lock()
        && let Some(stored) = cached.as_ref()
    {
        let object = v8::Local::new(scope, stored);
        return Some(finished(scope, shape, object));
    }

    let object = v8::Object::new(scope);
    let prototype = v8::Local::new(scope, &shape.prototype);
    object.set_prototype(scope, prototype.into())?;

    let state = from_json(scope, value).ok()?;
    let key = private_key(scope, STATE_KEY)?;
    object.set_private(scope, key, state);

    if let Some(brand) = &shape.brand {
        let marker = v8::String::new(scope, brand)?;
        let key = private_key(scope, BRAND_KEY)?;
        object.set_private(scope, key, marker.into());
    }

    if shape.singleton
        && let Ok(mut cached) = shape.cached.lock()
        && cached.is_none()
    {
        *cached = Some(v8::Global::new(scope, object));
    }

    Some(finished(scope, shape, object))
}

fn rejection<'s>(scope: &mut v8::PinScope<'s, '_>, text: &str) -> Option<v8::Local<'s, v8::Value>> {
    let resolver = v8::PromiseResolver::new(scope)?;
    let message = v8::String::new(scope, text)?;
    let exception = v8::Exception::type_error(scope, message);
    let promise = resolver.get_promise(scope);
    resolver.reject(scope, exception);
    Some(promise.into())
}

fn finished<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    shape: &ShapeEntry,
    object: v8::Local<'s, v8::Object>,
) -> v8::Local<'s, v8::Value> {
    if !shape.promise {
        return object.into();
    }

    match v8::PromiseResolver::new(scope) {
        Some(resolver) => {
            let promise = resolver.get_promise(scope);
            resolver.resolve(scope, object.into());
            promise.into()
        }
        None => object.into(),
    }
}

fn carries_brand(scope: &mut v8::PinScope, receiver: v8::Local<v8::Object>, brand: &str) -> bool {
    let Some(key) = v8::String::new(scope, BRAND_KEY) else {
        return false;
    };
    let private = v8::Private::for_api(scope, Some(key));

    let mut current = Some(receiver);
    let mut hops = 0;

    while let Some(object) = current {
        if hops > 16 {
            return false;
        }
        hops += 1;

        if let Some(found) = object.get_private(scope, private)
            && !found.is_undefined()
            && found.to_rust_string_lossy(scope) == brand
        {
            return true;
        }

        current = object
            .get_prototype(scope)
            .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok());
    }

    false
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

    if first.is_empty() {
        message
    } else {
        format!("{message} ({first})")
    }
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
                on: entry
                    .get("on")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                kind: entry
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                key: entry
                    .get("key")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }

    if let Some(list) = raw.get("errors").and_then(Value::as_array) {
        for entry in list {
            records.errors.push(ErrorRecord {
                where_: entry
                    .get("where")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                text: entry
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
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
                result: entry
                    .get("result")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                threw: entry
                    .get("threw")
                    .and_then(Value::as_str)
                    .map(str::to_string),
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
