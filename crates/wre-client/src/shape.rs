use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    Unit,
    Bool,
    Int,
    Float,
    Str,
    Bytes,
    Json,
    List { of: Box<Shape> },
    Map { of: Box<Shape> },
    Optional { of: Box<Shape> },
    Enum { name: String, variants: Vec<String> },
    Object { name: String, fields: Vec<Field> },
    Ref { name: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub shape: Shape,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
}

pub fn field(name: impl Into<String>, shape: Shape) -> Field {
    Field { name: name.into(), shape, summary: String::new(), default: None }
}

impl Field {
    pub fn summary(mut self, text: impl Into<String>) -> Self {
        self.summary = text.into();
        self
    }

    pub fn with_default(mut self, value: Value) -> Self {
        self.default = Some(value);
        self
    }

    pub fn required(&self) -> bool {
        self.default.is_none() && !self.shape.is_optional()
    }
}

impl Shape {
    pub fn list(of: Shape) -> Self {
        Shape::List { of: Box::new(of) }
    }

    pub fn map(of: Shape) -> Self {
        Shape::Map { of: Box::new(of) }
    }

    pub fn optional(of: Shape) -> Self {
        Shape::Optional { of: Box::new(of) }
    }

    pub fn object(name: impl Into<String>, fields: impl IntoIterator<Item = Field>) -> Self {
        Shape::Object { name: name.into(), fields: fields.into_iter().collect() }
    }

    pub fn enumeration(name: impl Into<String>, variants: &[&str]) -> Self {
        Shape::Enum {
            name: name.into(),
            variants: variants.iter().map(|item| item.to_string()).collect(),
        }
    }

    pub fn reference(name: impl Into<String>) -> Self {
        Shape::Ref { name: name.into() }
    }

    pub fn is_optional(&self) -> bool {
        matches!(self, Shape::Optional { .. })
    }

    pub fn inner(&self) -> &Shape {
        match self {
            Shape::Optional { of } | Shape::List { of } | Shape::Map { of } => of,
            other => other,
        }
    }

    pub fn type_name(&self) -> Option<&str> {
        match self {
            Shape::Object { name, .. } | Shape::Enum { name, .. } | Shape::Ref { name } => {
                Some(name)
            }
            _ => None,
        }
    }

    pub fn collect_types(&self, out: &mut IndexMap<String, Shape>) -> Result<(), String> {
        match self {
            Shape::List { of } | Shape::Map { of } | Shape::Optional { of } => {
                of.collect_types(out)
            }
            Shape::Enum { name, .. } => insert_type(name, self, out),
            Shape::Object { name, fields } => {
                if let Some(existing) = out.get(name) {
                    if existing != self {
                        return Err(format!("type {name} is declared twice with different fields"));
                    }
                    return Ok(());
                }
                out.insert(name.clone(), self.clone());
                for entry in fields {
                    entry.shape.collect_types(out)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

fn insert_type(name: &str, shape: &Shape, out: &mut IndexMap<String, Shape>) -> Result<(), String> {
    match out.get(name) {
        Some(existing) if existing != shape => {
            Err(format!("type {name} is declared twice with different definitions"))
        }
        Some(_) => Ok(()),
        None => {
            out.insert(name.to_string(), shape.clone());
            Ok(())
        }
    }
}

pub fn validate(shape: &Shape, value: &Value, types: &IndexMap<String, Shape>) -> Vec<String> {
    let mut problems = Vec::new();
    check(shape, value, "", types, &mut problems);
    problems
}

fn check(
    shape: &Shape,
    value: &Value,
    path: &str,
    types: &IndexMap<String, Shape>,
    problems: &mut Vec<String>,
) {
    let at = if path.is_empty() { "value".to_string() } else { path.to_string() };

    match shape {
        Shape::Unit => {
            if !value.is_null() {
                problems.push(format!("{at} should be null"));
            }
        }
        Shape::Bool => {
            if !value.is_boolean() {
                problems.push(format!("{at} should be a boolean, found {}", kind_of(value)));
            }
        }
        Shape::Int => {
            let integral = value.as_i64().is_some()
                || value.as_u64().is_some()
                || value.as_f64().is_some_and(|number| number.fract() == 0.0);
            if !integral {
                problems.push(format!("{at} should be an integer, found {}", kind_of(value)));
            }
        }
        Shape::Float => {
            if !value.is_number() {
                problems.push(format!("{at} should be a number, found {}", kind_of(value)));
            }
        }
        Shape::Str => {
            if !value.is_string() {
                problems.push(format!("{at} should be a string, found {}", kind_of(value)));
            }
        }
        Shape::Bytes => match value.as_str() {
            Some(text) => {
                use base64::Engine;
                if base64::engine::general_purpose::STANDARD.decode(text).is_err() {
                    problems.push(format!("{at} should be base64"));
                }
            }
            None => problems.push(format!("{at} should be a base64 string, found {}", kind_of(value))),
        },
        Shape::Json => {}
        Shape::Optional { of } => {
            if !value.is_null() {
                check(of, value, path, types, problems);
            }
        }
        Shape::List { of } => match value.as_array() {
            Some(items) => {
                for (index, item) in items.iter().enumerate() {
                    check(of, item, &format!("{at}[{index}]"), types, problems);
                }
            }
            None => problems.push(format!("{at} should be a list, found {}", kind_of(value))),
        },
        Shape::Map { of } => match value.as_object() {
            Some(entries) => {
                for (key, item) in entries {
                    check(of, item, &format!("{at}.{key}"), types, problems);
                }
            }
            None => problems.push(format!("{at} should be an object, found {}", kind_of(value))),
        },
        Shape::Enum { name, variants } => match value.as_str() {
            Some(text) if variants.iter().any(|variant| variant == text) => {}
            Some(text) => problems.push(format!(
                "{at} is {text}, which is not one of {name}: {}",
                variants.join(", ")
            )),
            None => problems.push(format!("{at} should be a string, found {}", kind_of(value))),
        },
        Shape::Ref { name } => match types.get(name) {
            Some(resolved) => check(resolved, value, path, types, problems),
            None => problems.push(format!("{at} refers to unknown type {name}")),
        },
        Shape::Object { name, fields } => {
            let Some(entries) = value.as_object() else {
                problems.push(format!("{at} should be a {name} object, found {}", kind_of(value)));
                return;
            };

            for entry in fields {
                let child = if path.is_empty() {
                    entry.name.clone()
                } else {
                    format!("{path}.{}", entry.name)
                };

                match entries.get(&entry.name) {
                    Some(found) => check(&entry.shape, found, &child, types, problems),
                    None if entry.required() => problems.push(format!("{child} is required")),
                    None => {}
                }
            }

            for key in entries.keys() {
                if !fields.iter().any(|entry| &entry.name == key) {
                    problems.push(format!("{at} has an unknown field {key}"));
                }
            }
        }
    }
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

pub fn apply_defaults(shape: &Shape, value: &mut Value, types: &IndexMap<String, Shape>) {
    match shape {
        Shape::Ref { name } => {
            if let Some(resolved) = types.get(name).cloned() {
                apply_defaults(&resolved, value, types);
            }
        }
        Shape::Optional { of } => {
            if !value.is_null() {
                apply_defaults(of, value, types);
            }
        }
        Shape::List { of } => {
            if let Some(items) = value.as_array_mut() {
                for item in items {
                    apply_defaults(of, item, types);
                }
            }
        }
        Shape::Map { of } => {
            if let Some(entries) = value.as_object_mut() {
                for (_, item) in entries.iter_mut() {
                    apply_defaults(of, item, types);
                }
            }
        }
        Shape::Object { fields, .. } => {
            if value.is_null() {
                *value = Value::Object(serde_json::Map::new());
            }
            let Some(entries) = value.as_object_mut() else {
                return;
            };
            for entry in fields {
                if !entries.contains_key(&entry.name) {
                    if let Some(default) = &entry.default {
                        entries.insert(entry.name.clone(), default.clone());
                    }
                }
            }
            for entry in fields {
                if let Some(found) = entries.get_mut(&entry.name) {
                    apply_defaults(&entry.shape, found, types);
                }
            }
        }
        _ => {}
    }
}

pub fn encode_bytes(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn decode_bytes(text: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|error| format!("base64 rejected: {error}"))
}
