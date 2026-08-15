use std::fmt::Write as _;
use std::path::PathBuf;

use wre_client::shape::Shape;
use wre_client::spec::OpSpec;
use wre_core::error::Result;

use crate::names::{pascal, snake};
use crate::{Language, Plan, json_string, summary_line, write};

pub fn emit(plan: &Plan) -> Result<Vec<PathBuf>> {
    let root = plan.root(Language::Rust);
    let mut files = Vec::new();

    files.push(write(&root.join("Cargo.toml"), &cargo_toml(plan))?);
    files.push(write(&root.join("src").join("lib.rs"), &lib_rs(plan))?);
    files.push(write(&root.join("README.md"), &readme(plan))?);

    Ok(files)
}

fn crate_name(plan: &Plan) -> String {
    format!("{}{}", plan.config.rust_prefix, plan.client.id.replace('_', "-"))
}

fn cargo_toml(plan: &Plan) -> String {
    let dependency = match &plan.config.rust_runtime_path {
        Some(path) => format!("wre-client = {{ path = {} }}", json_string(path)),
        None => format!("wre-client = {}", json_string(&plan.config.version)),
    };

    format!(
        r#"[package]
name = "{name}"
version = "{version}"
edition = "2024"
rust-version = "1.85"
license = "{license}"
repository = "{repository}"
description = "{description}"

[workspace]

[dependencies]
{dependency}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#,
        name = crate_name(plan),
        version = plan.config.version,
        license = plan.config.license,
        repository = plan.config.repository,
        description = summary_line(
            &plan.client.summary,
            &format!("Typed rust client for the {} target", plan.client.id)
        ),
    )
}

fn lib_rs(plan: &Plan) -> String {
    let mut out = String::new();

    out.push_str("use std::collections::BTreeMap;\n");
    out.push_str("use std::path::PathBuf;\n");
    out.push_str("use std::sync::Arc;\n");
    out.push_str("use std::time::Duration;\n\n");
    out.push_str("use serde::{Deserialize, Serialize};\n");
    out.push_str("use serde_json::{Value, json};\n\n");
    out.push_str("use wre_client::error::{ClientError, ClientResult};\n");
    out.push_str("use wre_client::sidecar::{EventHandler, Session, Sidecar, SidecarOptions};\n\n");

    let _ = writeln!(out, "pub const TARGET: &str = {};", json_string(&plan.client.id));
    let _ = writeln!(out, "pub const BUNDLE: &str = {};", json_string(&plan.bundle.bundle));
    let _ = writeln!(
        out,
        "pub const CLIENT_VERSION: &str = {};",
        json_string(&plan.client.version)
    );
    let _ = writeln!(
        out,
        "pub const BINARY_VERSION: &str = {};",
        json_string(&plan.bundle.binary_version)
    );
    let _ = writeln!(
        out,
        "pub const SCHEMA_HASH: &str = {};\n",
        json_string(&plan.schema_hash())
    );

    for (name, shape) in &plan.client.types {
        emit_type(&mut out, name, shape);
    }

    out.push_str("pub struct OpenOptions {\n");
    out.push_str("    pub binary: Option<PathBuf>,\n");
    out.push_str("    pub args: Vec<String>,\n");
    out.push_str("    pub env: Vec<(String, String)>,\n");
    out.push_str("    pub workspace: Option<PathBuf>,\n");
    out.push_str("    pub events: Option<EventHandler>,\n");
    out.push_str("    pub check_schema: bool,\n");
    out.push_str("    pub startup_timeout: Duration,\n");
    out.push_str("    pub diag: Value,\n");
    out.push_str("}\n\n");

    out.push_str("impl Default for OpenOptions {\n");
    out.push_str("    fn default() -> Self {\n");
    out.push_str("        Self {\n");
    out.push_str("            binary: None,\n");
    out.push_str("            args: Vec::new(),\n");
    out.push_str("            env: Vec::new(),\n");
    out.push_str("            workspace: None,\n");
    out.push_str("            events: None,\n");
    out.push_str("            check_schema: true,\n");
    out.push_str("            startup_timeout: Duration::from_secs(30),\n");
    out.push_str("            diag: Value::Null,\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");

    out.push_str("pub struct Client {\n");
    out.push_str("    sidecar: Arc<Sidecar>,\n");
    out.push_str("    session: Session,\n");
    out.push_str("}\n\n");

    let config_type = rust_type(&plan.client.config);

    out.push_str("impl Client {\n");
    let _ = writeln!(
        out,
        "    pub fn open(config: &{config_type}, options: OpenOptions) -> ClientResult<Self> {{"
    );
    out.push_str("        let mut spawn = match options.binary {\n");
    out.push_str("            Some(path) => SidecarOptions::new(path),\n");
    out.push_str("            None => SidecarOptions::discover()?,\n");
    out.push_str("        };\n\n");
    out.push_str("        spawn.args = options.args;\n");
    out.push_str("        spawn.env = options.env;\n");
    out.push_str("        spawn.workspace = options.workspace;\n");
    out.push_str("        spawn.events = options.events;\n");
    out.push_str("        spawn.startup_timeout = options.startup_timeout;\n\n");
    out.push_str("        let sidecar = Sidecar::spawn(spawn)?;\n");
    out.push_str("        let hello = sidecar.hello();\n\n");
    out.push_str("        if options.check_schema && hello.schema_hash != SCHEMA_HASH {\n");
    out.push_str("            let _ = sidecar.kill();\n");
    out.push_str("            return Err(ClientError::protocol(format!(\n");
    out.push_str("                \"this package was generated from schema {SCHEMA_HASH} and the binary reports {}\",\n");
    out.push_str("                hello.schema_hash\n");
    out.push_str("            )));\n");
    out.push_str("        }\n\n");
    out.push_str("        let value = serde_json::to_value(config)\n");
    out.push_str("            .map_err(|error| ClientError::bad_input(format!(\"config did not serialise: {error}\")))?;\n\n");
    out.push_str("        let session = sidecar.open_with_diag(TARGET, value, options.diag)?;\n\n");
    out.push_str("        Ok(Self { sidecar, session })\n");
    out.push_str("    }\n\n");

    let _ = writeln!(
        out,
        "    pub fn attach(sidecar: &Arc<Sidecar>, config: &{config_type}) -> ClientResult<Self> {{"
    );
    out.push_str("        let value = serde_json::to_value(config)\n");
    out.push_str("            .map_err(|error| ClientError::bad_input(format!(\"config did not serialise: {error}\")))?;\n\n");
    out.push_str("        let session = sidecar.open(TARGET, value)?;\n\n");
    out.push_str("        Ok(Self { sidecar: Arc::clone(sidecar), session })\n");
    out.push_str("    }\n\n");

    out.push_str("    pub fn sidecar(&self) -> &Arc<Sidecar> {\n");
    out.push_str("        &self.sidecar\n");
    out.push_str("    }\n\n");
    out.push_str("    pub fn session(&self) -> &Session {\n");
    out.push_str("        &self.session\n");
    out.push_str("    }\n\n");

    for op in &plan.client.ops {
        emit_method(&mut out, op);
    }

    out.push_str("    pub fn warmup(&self) -> ClientResult<Value> {\n");
    out.push_str("        self.session.warmup()\n");
    out.push_str("    }\n\n");
    out.push_str("    pub fn health(&self) -> ClientResult<Value> {\n");
    out.push_str("        let reply = self.session.health()?;\n");
    out.push_str("        serde_json::to_value(reply)\n");
    out.push_str("            .map_err(|error| ClientError::internal(format!(\"health did not serialise: {error}\")))\n");
    out.push_str("    }\n\n");
    out.push_str("    pub fn diagnose(&self, write: bool) -> ClientResult<Value> {\n");
    out.push_str("        let reply = self.session.diag(write)?;\n");
    out.push_str("        serde_json::to_value(reply)\n");
    out.push_str("            .map_err(|error| ClientError::internal(format!(\"diag did not serialise: {error}\")))\n");
    out.push_str("    }\n\n");
    out.push_str("    pub fn metrics(&self) -> ClientResult<Value> {\n");
    out.push_str("        self.sidecar.metrics()\n");
    out.push_str("    }\n\n");
    out.push_str("    pub fn close(self) -> ClientResult<()> {\n");
    out.push_str("        self.session.close()\n");
    out.push_str("    }\n");
    out.push_str("}\n\n");

    out.push_str("fn decode<T: serde::de::DeserializeOwned>(op: &str, value: Value) -> ClientResult<T> {\n");
    out.push_str("    serde_json::from_value(value).map_err(|error| {\n");
    out.push_str("        ClientError::protocol(format!(\"{op} returned a shape this package does not know: {error}\"))\n");
    out.push_str("    })\n");
    out.push_str("}\n\n");

    out.push_str("fn encode<T: Serialize>(op: &str, value: &T) -> ClientResult<Value> {\n");
    out.push_str("    serde_json::to_value(value)\n");
    out.push_str("        .map_err(|error| ClientError::bad_input(format!(\"{op} params did not serialise: {error}\")))\n");
    out.push_str("}\n");

    out
}

fn emit_method(out: &mut String, op: &OpSpec) {
    let method = escape_keyword(&snake(&op.name));
    let returns = rust_type(&op.returns);
    let takes_params = has_params(&op.params);

    if takes_params {
        let params = rust_type(&op.params);
        let _ = writeln!(
            out,
            "    pub fn {method}(&self, params: &{params}) -> ClientResult<{returns}> {{"
        );
        let _ = writeln!(out, "        let value = encode({}, params)?;", json_string(&op.name));
        let _ = writeln!(
            out,
            "        let result = self.session.call({}, value)?;",
            json_string(&op.name)
        );
    } else {
        let _ = writeln!(out, "    pub fn {method}(&self) -> ClientResult<{returns}> {{");
        let _ = writeln!(
            out,
            "        let result = self.session.call({}, json!({{}}))?;",
            json_string(&op.name)
        );
    }

    let _ = writeln!(out, "        decode({}, result)", json_string(&op.name));
    out.push_str("    }\n\n");
}

fn emit_type(out: &mut String, name: &str, shape: &Shape) {
    match shape {
        Shape::Enum { variants, .. } => {
            out.push_str("#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]\n");
            let _ = writeln!(out, "pub enum {name} {{");
            for variant in variants {
                let ident = pascal(variant);
                if ident == *variant {
                    let _ = writeln!(out, "    {ident},");
                } else {
                    let _ = writeln!(out, "    #[serde(rename = {})]", json_string(variant));
                    let _ = writeln!(out, "    {ident},");
                }
            }
            out.push_str("}\n\n");
        }
        Shape::Object { fields, .. } => {
            out.push_str("#[derive(Debug, Clone, Default, Serialize, Deserialize)]\n");
            let _ = writeln!(out, "pub struct {name} {{");

            for entry in fields {
                let ident = escape_keyword(&snake(&entry.name));
                let optional = !entry.required();
                let base = rust_type(&entry.shape);
                let kind = if optional && !entry.shape.is_optional() {
                    format!("Option<{base}>")
                } else {
                    base
                };

                if ident != entry.name {
                    let _ = writeln!(out, "    #[serde(rename = {})]", json_string(&entry.name));
                }

                if kind.starts_with("Option<") {
                    out.push_str("    #[serde(default, skip_serializing_if = \"Option::is_none\")]\n");
                }

                let _ = writeln!(out, "    pub {ident}: {kind},");
            }

            out.push_str("}\n\n");
        }
        _ => {}
    }
}

fn has_params(shape: &Shape) -> bool {
    match shape {
        Shape::Object { fields, .. } => !fields.is_empty(),
        Shape::Unit => false,
        _ => true,
    }
}

fn rust_type(shape: &Shape) -> String {
    match shape {
        Shape::Unit => "()".to_string(),
        Shape::Bool => "bool".to_string(),
        Shape::Int => "i64".to_string(),
        Shape::Float => "f64".to_string(),
        Shape::Str | Shape::Bytes => "String".to_string(),
        Shape::Json => "Value".to_string(),
        Shape::List { of } => format!("Vec<{}>", rust_type(of)),
        Shape::Map { of } => format!("BTreeMap<String, {}>", rust_type(of)),
        Shape::Optional { of } => format!("Option<{}>", rust_type(of)),
        Shape::Enum { name, .. } | Shape::Object { name, .. } | Shape::Ref { name } => {
            name.to_string()
        }
    }
}

fn escape_keyword(name: &str) -> String {
    const KEYWORDS: [&str; 38] = [
        "as", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false",
        "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
        "ref", "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe",
        "use", "where", "while", "async", "await", "box",
    ];

    if KEYWORDS.contains(&name) { format!("r#{name}") } else { name.to_string() }
}

fn readme(plan: &Plan) -> String {
    let name = crate_name(plan);
    let config_type = rust_type(&plan.client.config);

    let first = plan
        .client
        .ops
        .iter()
        .find(|op| has_params(&op.params))
        .or_else(|| plan.client.ops.first());

    let call = match first {
        Some(op) if has_params(&op.params) => format!(
            "let result = client.{}(&{}::default())?;",
            escape_keyword(&snake(&op.name)),
            rust_type(&op.params)
        ),
        Some(op) => format!("let result = client.{}()?;", escape_keyword(&snake(&op.name))),
        None => "let result = client.health()?;".to_string(),
    };

    format!(
        r#"# {name}

Generated rust client for the `{id}` target. It drives a `wred` sidecar over a pipe.

{summary}

If you have the client crate in your own workspace, depend on it directly and skip the sidecar.

## Use

```rust
use {lib}::{{Client, OpenOptions, {config_type}}};

let client = Client::open(&{config_type}::default(), OpenOptions::default())?;
{call}
println!("{{result:?}}");
client.close()?;
```

The binary is found through `WRE_BINARY`, then `WRE_WRED`, then `target/release/wred` upward from the working directory, then `PATH`.

## Pinned build

- bundle `{bundle}`
- binary version `{binary_version}`
- schema hash `{schema_hash}`

`OpenOptions::check_schema` is on by default and fails the open when the binary reports a different surface.

## Diagnostics

`client.diagnose(true)` writes one json report holding the session's calls, the host facts and the target specific debug section, and returns the path.
"#,
        id = plan.client.id,
        lib = name.replace('-', "_"),
        summary = summary_line(&plan.client.summary, "No summary was declared for this target."),
        bundle = plan.bundle.bundle,
        binary_version = plan.bundle.binary_version,
        schema_hash = plan.schema_hash(),
    )
}
