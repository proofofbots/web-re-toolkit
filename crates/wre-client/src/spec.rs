use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::shape::{Field, Shape};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub needs_v8: bool,
    #[serde(default)]
    pub needs_chrome: bool,
    #[serde(default)]
    pub needs_network: bool,
    #[serde(default)]
    pub stateful: bool,
    #[serde(default)]
    pub concurrency: Concurrency,
    #[serde(default)]
    pub warmup_ms: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Concurrency {
    #[default]
    PerSession,
    Shared,
    SingleThread,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    pub params: Shape,
    pub returns: Shape,
    #[serde(default)]
    pub streams: Vec<String>,
    #[serde(default)]
    pub deadline_ms: u64,
}

impl OpSpec {
    pub fn new(name: impl Into<String>, params: Shape, returns: Shape) -> Self {
        Self {
            name: name.into(),
            summary: String::new(),
            params,
            returns,
            streams: Vec::new(),
            deadline_ms: 0,
        }
    }

    pub fn summary(mut self, text: impl Into<String>) -> Self {
        self.summary = text.into();
        self
    }

    pub fn deadline_ms(mut self, value: u64) -> Self {
        self.deadline_ms = value;
        self
    }

    pub fn streams(mut self, events: &[&str]) -> Self {
        self.streams = events.iter().map(|name| name.to_string()).collect();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    pub data: Shape,
}

impl EventSpec {
    pub fn new(name: impl Into<String>, data: Shape) -> Self {
        Self { name: into_string(name), summary: String::new(), data }
    }

    pub fn summary(mut self, text: impl Into<String>) -> Self {
        self.summary = text.into();
        self
    }
}

fn into_string(value: impl Into<String>) -> String {
    value.into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientDescriptor {
    pub id: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<String>,
    #[serde(default)]
    pub capabilities: Capabilities,
    pub config: Shape,
    pub ops: Vec<OpSpec>,
    #[serde(default)]
    pub events: Vec<EventSpec>,
    #[serde(default)]
    pub types: IndexMap<String, Shape>,
}

impl ClientDescriptor {
    pub fn new(id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            version: version.into(),
            summary: String::new(),
            notes: String::new(),
            primary: None,
            capabilities: Capabilities::default(),
            config: Shape::object("Config", Vec::<Field>::new()),
            ops: Vec::new(),
            events: Vec::new(),
            types: IndexMap::new(),
        }
    }

    pub fn summary(mut self, text: impl Into<String>) -> Self {
        self.summary = text.into();
        self
    }

    /// Markdown that lands in every generated package README, under the quickstart.
    pub fn notes(mut self, text: impl Into<String>) -> Self {
        self.notes = text.into();
        self
    }

    /// The op the generated examples call. Defaults to the first op that takes arguments.
    pub fn primary(mut self, op: impl Into<String>) -> Self {
        self.primary = Some(op.into());
        self
    }

    pub fn capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn config(mut self, shape: Shape) -> Self {
        self.config = shape;
        self
    }

    pub fn op(mut self, spec: OpSpec) -> Self {
        self.ops.push(spec);
        self
    }

    pub fn event(mut self, spec: EventSpec) -> Self {
        self.events.push(spec);
        self
    }

    pub fn find(&self, op: &str) -> Option<&OpSpec> {
        self.ops.iter().find(|entry| entry.name == op)
    }

    pub fn seal(mut self) -> Result<Self, String> {
        let mut types = IndexMap::new();
        self.config.collect_types(&mut types)?;

        let mut seen = Vec::new();
        for op in &self.ops {
            if seen.contains(&op.name) {
                return Err(format!("op {} is declared twice", op.name));
            }
            if !is_identifier(&op.name) {
                return Err(format!("op {} is not a usable identifier", op.name));
            }
            seen.push(op.name.clone());
            op.params.collect_types(&mut types)?;
            op.returns.collect_types(&mut types)?;
        }

        for event in &self.events {
            event.data.collect_types(&mut types)?;
        }

        for op in &self.ops {
            for event in &op.streams {
                if !self.events.iter().any(|entry| &entry.name == event) {
                    return Err(format!("op {} streams undeclared event {event}", op.name));
                }
            }
        }

        if let Some(primary) = &self.primary {
            if !seen.contains(primary) {
                return Err(format!("primary op {primary} is not declared"));
            }
        }

        self.types = types;
        Ok(self)
    }
}

fn is_identifier(name: &str) -> bool {
    !name.is_empty()
        && name.chars().next().is_some_and(|first| first.is_ascii_alphabetic())
        && name.chars().all(|item| item.is_ascii_alphanumeric() || item == '_')
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BundleDescriptor {
    pub protocol: u32,
    pub bundle: String,
    pub toolkit_version: String,
    pub binary_version: String,
    pub clients: Vec<ClientDescriptor>,
}

impl BundleDescriptor {
    pub fn find(&self, id: &str) -> Option<&ClientDescriptor> {
        self.clients.iter().find(|entry| entry.id == id)
    }

    pub fn schema_hash(&self) -> String {
        let surface: Vec<_> = self
            .clients
            .iter()
            .map(|client| {
                serde_json::json!({
                    "id": client.id,
                    "config": client.config,
                    "ops": client.ops,
                    "events": client.events,
                    "types": client.types,
                })
            })
            .collect();

        let encoded = serde_json::to_vec(&serde_json::json!({
            "protocol": self.protocol,
            "clients": surface,
        }))
        .unwrap_or_default();

        wre_core::digest::sha256_short(&encoded)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub bundle: String,
    pub binary_version: String,
    pub toolkit_version: String,
    pub schema_hash: String,
    pub targets: Vec<String>,
    pub workers: usize,
    pub pid: u32,
}
