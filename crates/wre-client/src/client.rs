use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::context::{Call, Ctx};
use crate::error::{ClientError, ClientResult};
use crate::shape::{Shape, apply_defaults, validate};
use crate::spec::ClientDescriptor;

pub trait Client {
    fn call(&mut self, op: &str, params: Value, call: &Call) -> ClientResult<Value>;

    fn warmup(&mut self, _call: &Call) -> ClientResult<()> {
        Ok(())
    }

    fn health(&mut self) -> ClientResult<Value> {
        Ok(json!({ "ok": true }))
    }

    fn diagnostics(&mut self) -> Value {
        Value::Null
    }

    fn close(&mut self) -> ClientResult<()> {
        Ok(())
    }
}

pub type BuildFn = fn(Ctx, Value) -> ClientResult<Box<dyn Client>>;
pub type DescribeFn = fn() -> ClientDescriptor;

#[derive(Clone, Copy)]
pub struct Registration {
    pub id: &'static str,
    pub describe: DescribeFn,
    pub build: BuildFn,
}

#[derive(Default)]
pub struct Registry {
    builders: BTreeMap<String, BuildFn>,
    descriptors: BTreeMap<String, ClientDescriptor>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, entry: Registration) -> Result<(), String> {
        let descriptor = (entry.describe)().seal()?;

        if descriptor.id != entry.id {
            return Err(format!(
                "registration id {} does not match descriptor id {}",
                entry.id, descriptor.id
            ));
        }

        if self.descriptors.contains_key(entry.id) {
            return Err(format!("target {} is registered twice", entry.id));
        }

        self.builders.insert(entry.id.to_string(), entry.build);
        self.descriptors.insert(entry.id.to_string(), descriptor);
        Ok(())
    }

    pub fn ids(&self) -> Vec<String> {
        self.descriptors.keys().cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }

    pub fn descriptor(&self, id: &str) -> Option<&ClientDescriptor> {
        self.descriptors.get(id)
    }

    pub fn descriptors(&self) -> Vec<ClientDescriptor> {
        self.descriptors.values().cloned().collect()
    }

    pub fn build(&self, id: &str, ctx: Ctx, config: Value) -> ClientResult<Box<dyn Client>> {
        let descriptor = self.descriptors.get(id).ok_or_else(|| {
            ClientError::unsupported(format!(
                "target {id} is not in this build, it has {}",
                describe_ids(&self.ids())
            ))
        })?;

        let build = self
            .builders
            .get(id)
            .ok_or_else(|| ClientError::unsupported(format!("target {id} has no builder")))?;

        let config = prepare(descriptor, &descriptor.config, config, "config")
            .map_err(|error| error.with_target(id))?;

        build(ctx, config).map_err(|error| error.with_target(id))
    }
}

fn describe_ids(ids: &[String]) -> String {
    if ids.is_empty() { "none".to_string() } else { ids.join(", ") }
}

pub fn prepare(
    descriptor: &ClientDescriptor,
    shape: &Shape,
    mut value: Value,
    what: &str,
) -> ClientResult<Value> {
    if value.is_null() {
        value = json!({});
    }

    apply_defaults(shape, &mut value, &descriptor.types);

    let problems = validate(shape, &value, &descriptor.types);
    if problems.is_empty() {
        return Ok(value);
    }

    Err(ClientError::bad_input(format!("{what} rejected: {}", problems.join("; ")))
        .with_detail(json!({ "problems": problems })))
}

pub fn prepare_params(
    descriptor: &ClientDescriptor,
    op: &str,
    params: Value,
) -> ClientResult<Value> {
    let spec = descriptor.find(op).ok_or_else(|| {
        let known: Vec<&str> = descriptor.ops.iter().map(|entry| entry.name.as_str()).collect();
        ClientError::unsupported(format!(
            "{} has no op {op}, it has {}",
            descriptor.id,
            if known.is_empty() { "none".to_string() } else { known.join(", ") }
        ))
    })?;

    prepare(descriptor, &spec.params, params, &format!("params for {op}"))
        .map_err(|error| error.with_op(op).with_target(&descriptor.id))
}
