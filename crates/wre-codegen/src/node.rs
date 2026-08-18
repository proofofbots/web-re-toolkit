use std::fmt::Write as _;
use std::path::PathBuf;

use wre_client::shape::Shape;
use wre_client::spec::OpSpec;
use wre_core::error::Result;

use crate::names::{binary_name, camel, npm_platform, pascal};
use crate::reference::{self, Sample, Style, Words};
use crate::{Language, Plan, copy_binary, json_string, summary_line, write};

pub fn emit(plan: &Plan) -> Result<Vec<PathBuf>> {
    let root = plan.root(Language::Node);
    let mut files = Vec::new();

    files.push(write(&root.join("package.json"), &package_json(plan))?);
    files.push(write(&root.join("index.js"), &index_js(plan))?);
    files.push(write(&root.join("index.d.ts"), &index_dts(plan))?);
    files.push(write(&root.join("README.md"), &readme(plan))?);

    for entry in &plan.binaries.entries {
        let Some((tag, os, cpu)) = npm_platform(&entry.triple) else {
            continue;
        };

        let dir = root.join("platform").join(tag);
        files.push(write(
            &dir.join("package.json"),
            &platform_package_json(plan, tag, os, cpu),
        )?);
        files.push(copy_binary(&entry.path, &dir.join("bin").join(binary_name(&entry.triple)))?);
    }

    Ok(files)
}

fn package_name(plan: &Plan) -> String {
    format!("{}/client-{}", plan.config.node_scope, plan.client.id)
}

fn platform_package_name(plan: &Plan, tag: &str) -> String {
    format!("{}-{tag}", package_name(plan))
}

fn package_json(plan: &Plan) -> String {
    let mut optional = String::new();
    for entry in &plan.binaries.entries {
        let Some((tag, _, _)) = npm_platform(&entry.triple) else {
            continue;
        };
        if !optional.is_empty() {
            optional.push_str(",\n");
        }
        let _ = write!(
            optional,
            "    {}: {}",
            json_string(&platform_package_name(plan, tag)),
            json_string(&plan.config.version)
        );
    }

    let optional_block = if optional.is_empty() {
        String::new()
    } else {
        format!(",\n  \"optionalDependencies\": {{\n{optional}\n  }}")
    };

    format!(
        r#"{{
  "name": {name},
  "version": {version},
  "description": {description},
  "license": {license},
  "repository": {{
    "type": "git",
    "url": {repository}
  }},
  "type": "module",
  "main": "./index.js",
  "types": "./index.d.ts",
  "exports": {{
    ".": {{
      "types": "./index.d.ts",
      "import": "./index.js"
    }},
    "./package.json": "./package.json"
  }},
  "engines": {{
    "node": ">=18"
  }},
  "files": [
    "index.js",
    "index.d.ts",
    "README.md"
  ],
  "dependencies": {{
    {runtime_name}: {runtime_version}
  }}{optional_block}
}}
"#,
        name = json_string(&package_name(plan)),
        version = json_string(&plan.config.version),
        description = json_string(&summary_line(
            &plan.client.summary,
            &format!("Headless client for {}", plan.client.id)
        )),
        license = json_string(&plan.config.license),
        repository = json_string(&git_repository_url(&plan.config.repository)),
        runtime_name = json_string(&format!("{}/runtime", plan.config.node_scope)),
        runtime_version = json_string(&plan.config.node_runtime),
    )
}

fn platform_package_json(plan: &Plan, tag: &str, os: &str, cpu: &str) -> String {
    format!(
        r#"{{
  "name": {name},
  "version": {version},
  "description": {description},
  "license": {license},
  "repository": {{
    "type": "git",
    "url": {repository}
  }},
  "os": ["{os}"],
  "cpu": ["{cpu}"],
  "files": ["bin"]
}}
"#,
        name = json_string(&platform_package_name(plan, tag)),
        version = json_string(&plan.config.version),
        description = json_string(&format!("wred binary for {tag}")),
        license = json_string(&plan.config.license),
        repository = json_string(&git_repository_url(&plan.config.repository)),
    )
}

fn git_repository_url(repository: &str) -> String {
    let trimmed = repository.trim_end_matches('/');
    if trimmed.starts_with("git+") {
        return trimmed.to_string();
    }
    if trimmed.ends_with(".git") {
        return format!("git+{trimmed}");
    }
    format!("git+{trimmed}.git")
}

fn index_js(plan: &Plan) -> String {
    let class_name = format!("{}Client", pascal(&plan.client.id));
    let mut out = String::new();

    out.push_str("import { createRequire } from \"node:module\";\n");
    out.push_str("import { dirname, join } from \"node:path\";\n");
    out.push_str(
        "import { connect as connectSidecar, currentTriple, resolveBinary, WreError } from ",
    );
    let _ = writeln!(out, "{};\n", json_string(&format!("{}/runtime", plan.config.node_scope)));

    let _ = writeln!(out, "export const TARGET = {};", json_string(&plan.client.id));
    let _ = writeln!(out, "export const BUNDLE = {};", json_string(&plan.bundle.bundle));
    let _ = writeln!(
        out,
        "export const CLIENT_VERSION = {};",
        json_string(&plan.client.version)
    );
    let _ = writeln!(
        out,
        "export const BINARY_VERSION = {};",
        json_string(&plan.bundle.binary_version)
    );
    let _ = writeln!(out, "export const SCHEMA_HASH = {};\n", json_string(&plan.schema_hash()));

    out.push_str("const PLATFORMS = {\n");
    for entry in &plan.binaries.entries {
        let Some((tag, _, _)) = npm_platform(&entry.triple) else {
            continue;
        };
        let _ = writeln!(
            out,
            "  {}: {{ package: {}, sha256: {} }},",
            json_string(&entry.triple),
            json_string(&platform_package_name(plan, tag)),
            json_string(&entry.sha256)
        );
    }
    out.push_str("};\n\n");

    out.push_str("const require_ = createRequire(import.meta.url);\n\n");

    out.push_str("export function binaryPath() {\n");
    out.push_str("  if (process.env.WRE_BINARY) {\n");
    out.push_str("    return resolveBinary({});\n");
    out.push_str("  }\n\n");
    out.push_str("  const triple = currentTriple();\n");
    out.push_str("  const platform = PLATFORMS[triple];\n");
    out.push_str("  if (!platform) {\n");
    out.push_str("    throw new WreError(\"unsupported\", `this package ships no binary for ${triple}, set WRE_BINARY to a wred you built`);\n");
    out.push_str("  }\n\n");
    out.push_str("  let entry;\n");
    out.push_str("  try {\n");
    out.push_str("    entry = require_.resolve(`${platform.package}/package.json`);\n");
    out.push_str("  } catch (cause) {\n");
    out.push_str("    throw new WreError(\"resource\", `${platform.package} is not installed, reinstall without --no-optional or set WRE_BINARY`, { detail: { triple } });\n");
    out.push_str("  }\n\n");
    out.push_str("  return resolveBinary({ embedded: join(dirname(entry), \"bin\"), sha256: platform.sha256 });\n");
    out.push_str("}\n\n");

    let _ = writeln!(out, "export class {class_name} {{");
    out.push_str("  constructor(sidecar, session, owned) {\n");
    out.push_str("    this.sidecar = sidecar;\n");
    out.push_str("    this.session = session;\n");
    out.push_str("    this.owned = owned === true;\n");
    out.push_str("    this.closed = false;\n");
    out.push_str("  }\n\n");

    out.push_str("  static async open(config = {}, options = {}) {\n");
    out.push_str("    const sidecar = await connectSidecar({\n");
    out.push_str("      binary: options.binary ?? binaryPath(),\n");
    out.push_str("      args: options.args,\n");
    out.push_str("      env: options.env,\n");
    out.push_str("      cwd: options.cwd,\n");
    out.push_str("      stderr: options.stderr,\n");
    out.push_str("      onEvent: options.onEvent,\n");
    out.push_str("      expectSchemaHash: options.checkSchema === false ? undefined : SCHEMA_HASH,\n");
    out.push_str("      startupTimeoutMs: options.startupTimeoutMs,\n");
    out.push_str("    });\n\n");
    out.push_str("    try {\n");
    out.push_str("      const session = await sidecar.open(TARGET, config);\n");
    let _ = writeln!(out, "      return new {class_name}(sidecar, session, true);");
    out.push_str("    } catch (error) {\n");
    out.push_str("      await sidecar.close();\n");
    out.push_str("      throw error;\n");
    out.push_str("    }\n");
    out.push_str("  }\n\n");

    out.push_str("  static async attach(sidecar, config = {}) {\n");
    out.push_str("    const session = await sidecar.open(TARGET, config);\n");
    let _ = writeln!(out, "    return new {class_name}(sidecar, session, false);");
    out.push_str("  }\n\n");

    for op in &plan.client.ops {
        emit_method_js(&mut out, op);
    }

    out.push_str("  async warmup() {\n");
    out.push_str("    return this.session.warmup();\n");
    out.push_str("  }\n\n");
    out.push_str("  async health() {\n");
    out.push_str("    return this.session.health();\n");
    out.push_str("  }\n\n");
    out.push_str("  async metrics() {\n");
    out.push_str("    return this.sidecar.metrics();\n");
    out.push_str("  }\n\n");

    out.push_str("  async diagnose(write = true) {\n");
    out.push_str("    return this.session.call(\"diag\", { write, events: true });\n");
    out.push_str("  }\n\n");
    out.push_str("  async close() {\n");
    out.push_str("    if (this.closed) {\n");
    out.push_str("      return;\n");
    out.push_str("    }\n");
    out.push_str("    this.closed = true;\n");
    out.push_str("    await this.session.close();\n");
    out.push_str("    if (this.owned) {\n");
    out.push_str("      await this.sidecar.close();\n");
    out.push_str("    }\n");
    out.push_str("  }\n");
    out.push_str("}\n\n");

    let _ = writeln!(
        out,
        "export async function open(config = {{}}, options = {{}}) {{\n  return {class_name}.open(config, options);\n}}"
    );

    out.push_str("\nexport { WreError };\n");
    out
}

fn emit_method_js(out: &mut String, op: &OpSpec) {
    let method = camel(&op.name);
    let takes_params = has_params(&op.params);

    if takes_params {
        let _ = writeln!(out, "  async {method}(params, options = {{}}) {{");
        let _ = writeln!(
            out,
            "    return this.session.call({}, params ?? {{}}, options);",
            json_string(&op.name)
        );
    } else {
        let _ = writeln!(out, "  async {method}(options = {{}}) {{");
        let _ = writeln!(out, "    return this.session.call({}, {{}}, options);", json_string(&op.name));
    }

    out.push_str("  }\n\n");
}

fn has_params(shape: &Shape) -> bool {
    match shape {
        Shape::Object { fields, .. } => !fields.is_empty(),
        Shape::Unit => false,
        _ => true,
    }
}

fn resolve<'a>(plan: &'a Plan, shape: &'a Shape) -> &'a Shape {
    match shape {
        Shape::Ref { name } => plan.client.types.get(name).unwrap_or(shape),
        _ => shape,
    }
}

fn index_dts(plan: &Plan) -> String {
    let class_name = format!("{}Client", pascal(&plan.client.id));
    let mut out = String::new();

    let _ = writeln!(
        out,
        "import type {{ Session, Sidecar, ConnectOptions, CallOptions, WreError }} from {};\n",
        json_string(&format!("{}/runtime", plan.config.node_scope))
    );

    out.push_str("export declare const TARGET: string;\n");
    out.push_str("export declare const BUNDLE: string;\n");
    out.push_str("export declare const CLIENT_VERSION: string;\n");
    out.push_str("export declare const BINARY_VERSION: string;\n");
    out.push_str("export declare const SCHEMA_HASH: string;\n");
    out.push_str("export declare function binaryPath(): string;\n\n");

    for (name, shape) in &plan.client.types {
        emit_type_dts(&mut out, name, shape);
    }

    out.push_str("export interface OpenOptions extends Partial<ConnectOptions> {\n");
    out.push_str("  binary?: string;\n");
    out.push_str("  checkSchema?: boolean;\n");
    out.push_str("}\n\n");

    out.push_str("export interface DiagReport {\n");
    out.push_str("  target: string;\n");
    out.push_str("  session: string;\n");
    out.push_str("  mode: string;\n");
    out.push_str("  path?: string;\n");
    out.push_str("  report: unknown;\n");
    out.push_str("}\n\n");

    out.push_str("export interface HealthReply {\n");
    out.push_str("  ok: boolean;\n");
    out.push_str("  target: string;\n");
    out.push_str("  detail: unknown;\n");
    out.push_str("}\n\n");

    let config_type = ts_type(&plan.client.config);

    let _ = writeln!(out, "export declare class {class_name} {{");
    out.push_str("  readonly sidecar: Sidecar;\n");
    out.push_str("  readonly session: Session;\n");
    let _ = writeln!(
        out,
        "  static open(config?: {config_type}, options?: OpenOptions): Promise<{class_name}>;"
    );
    let _ = writeln!(
        out,
        "  static attach(sidecar: Sidecar, config?: {config_type}): Promise<{class_name}>;"
    );

    for op in &plan.client.ops {
        let method = camel(&op.name);
        let returns = ts_type(&op.returns);
        if has_params(&op.params) {
            let params = ts_type(&op.params);
            let _ = writeln!(
                out,
                "  {method}(params: {params}, options?: CallOptions): Promise<{returns}>;"
            );
        } else {
            let _ = writeln!(out, "  {method}(options?: CallOptions): Promise<{returns}>;");
        }
    }

    out.push_str("  warmup(): Promise<unknown>;\n");
    out.push_str("  health(): Promise<HealthReply>;\n");
    out.push_str("  metrics(): Promise<unknown>;\n");
    out.push_str("  diagnose(write?: boolean): Promise<DiagReport>;\n");
    out.push_str("  close(): Promise<void>;\n");
    out.push_str("}\n\n");

    let _ = writeln!(
        out,
        "export declare function open(config?: {config_type}, options?: OpenOptions): Promise<{class_name}>;"
    );
    out.push_str("export { WreError };\n");

    out
}

fn emit_type_dts(out: &mut String, name: &str, shape: &Shape) {
    match shape {
        Shape::Enum { variants, .. } => {
            let joined: Vec<String> = variants.iter().map(|item| json_string(item)).collect();
            let _ = writeln!(out, "export type {name} = {};\n", joined.join(" | "));
        }
        Shape::Object { fields, .. } => {
            let _ = writeln!(out, "export interface {name} {{");
            for entry in fields {
                let optional = if entry.required() { "" } else { "?" };
                let _ = writeln!(
                    out,
                    "  {}{}: {};",
                    quote_key(&entry.name),
                    optional,
                    ts_type(&entry.shape)
                );
            }
            out.push_str("}\n\n");
        }
        _ => {}
    }
}

fn quote_key(name: &str) -> String {
    let plain = !name.is_empty()
        && name.chars().next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && name.chars().all(|item| item.is_ascii_alphanumeric() || item == '_');

    if plain { name.to_string() } else { json_string(name) }
}

fn ts_type(shape: &Shape) -> String {
    match shape {
        Shape::Unit => "null".to_string(),
        Shape::Bool => "boolean".to_string(),
        Shape::Int | Shape::Float => "number".to_string(),
        Shape::Str | Shape::Bytes => "string".to_string(),
        Shape::Json => "unknown".to_string(),
        Shape::List { of } => format!("Array<{}>", ts_type(of)),
        Shape::Map { of } => format!("Record<string, {}>", ts_type(of)),
        Shape::Optional { of } => format!("{} | null", ts_type(of)),
        Shape::Enum { name, .. } | Shape::Object { name, .. } | Shape::Ref { name } => {
            name.to_string()
        }
    }
}

fn style<'a>() -> Style<'a> {
    Style {
        type_name: &ts_type,
        signature: &|op: &OpSpec| {
            if has_params(&op.params) {
                format!("`client.{}(params, options)`", camel(&op.name))
            } else {
                format!("`client.{}(options)`", camel(&op.name))
            }
        },
        words: Words { truth: "true", falsehood: "false" },
    }
}

fn readme(plan: &Plan) -> String {
    let class_name = format!("{}Client", pascal(&plan.client.id));
    let package = package_name(plan);
    let style = style();
    let notes = reference::notes_section(plan);
    let config = reference::config_section(plan, &style);
    let operations = reference::operations_section(plan, &style);
    let first = reference::primary_op(plan, &has_params);

    let call = match first {
        Some(op) if has_params(&op.params) => {
            format!("const result = await client.{}({});", camel(&op.name), sample(resolve(plan, &op.params)))
        }
        Some(op) => format!("const result = await client.{}();", camel(&op.name)),
        None => "const result = await client.health();".to_string(),
    };

    let call_options = match first {
        Some(op) if op.deadline_ms > 0 => {
            format!("{{ signal: abort.signal, deadlineMs: {} }}", op.deadline_ms)
        }
        _ => "{ signal: abort.signal }".to_string(),
    };

    let call_with_options = match first {
        Some(op) if has_params(&op.params) => format!(
            "await client.{}({}, {call_options});",
            camel(&op.name),
            sample(resolve(plan, &op.params))
        ),
        Some(op) => format!("await client.{}({call_options});", camel(&op.name)),
        None => "await client.health();".to_string(),
    };

    let streaming = plan
        .client
        .ops
        .iter()
        .find(|op| !op.streams.is_empty());

    let events = match (streaming, plan.client.events.first()) {
        (Some(op), Some(event)) => format!(
            r#"## Events

`{op_name}` streams `{event_name}` while it runs. A session is not an emitter, so pass `onEvent` when you open the client to see every event, or per call to scope it to one:

```js
const client = await {class_name}.open({{}}, {{
  onEvent: (id, event, data) => console.log(event, data),
}});

await client.{op_camel}({op_sample}, {{
  onEvent: (id, event, data) => console.log(event, data),
}});
```

{event_table}
"#,
            op_name = op.name,
            op_camel = camel(&op.name),
            op_sample = sample(resolve(plan, &op.params)),
            event_name = event.name,
            event_table = reference::events_table(plan, &style),
        ),
        _ => String::new(),
    };

    format!(
        r#"# {package}

Generated client for the `{id}` target. It talks to a `wred` sidecar over a pipe.

{summary}

## Install

```bash
npm install {package}
```

The binary for your platform arrives as an optional dependency. Set `WRE_BINARY` to an absolute path to override it with a local build.

## Use

```js
import {{ {class_name} }} from "{package}";

const client = await {class_name}.open({{}});
{call}
console.log(result);
await client.close();
```

One client owns one session, which owns the mounted realm. Open it once and reuse it. Opening one per call pays the warmup cost every time.

{notes}{config}{operations}{events}## Deadlines and cancellation

Every op takes a second options argument: `deadlineMs` caps the call and fails it with `kind === "timeout"`, `signal` takes an `AbortSignal` and fails it with `kind === "cancelled"`. Both stop the work inside the sidecar, they do not only abandon the promise.

```js
const abort = new AbortController();
setTimeout(() => abort.abort(), 5000);
{call_with_options}
```

## Errors

Every rejection is a `WreError` with a stable `kind`: `bad_input`, `unsupported`, `target_drift`, `blocked`, `timeout`, `cancelled`, `resource`, `protocol`, `internal`. Branch on `kind`, never on the message. `error.retryable` says whether the same call is worth repeating.

## Sidecar output and diagnostics

The sidecar logs to its own stderr, which is discarded by default. Pass `{{ stderr: "inherit" }}` to `open`, or set `WRE_STDERR=inherit`, to see it.

A failing call writes a diagnostic report and puts its path in `error.detail.diagnostics`. `WRE_DIAG=always` records every call, `WRE_DIAG=off` records none, and `await client.diagnose(true)` writes one on demand. Send that file with a bug report.

## Pinned build

- bundle `{bundle}`
- binary version `{binary_version}`
- schema hash `{schema_hash}`

The schema hash is checked at connect time. A mismatch means this package and the installed binary disagree about the callable surface, and the connect call fails.
"#,
        id = plan.client.id,
        summary = summary_line(&plan.client.summary, "No summary was declared for this target."),
        bundle = plan.bundle.bundle,
        binary_version = plan.bundle.binary_version,
        schema_hash = plan.schema_hash(),
    )
}

fn sample(shape: &Shape) -> String {
    let fields = reference::example_params(shape);
    if fields.is_empty() {
        return "{}".to_string();
    }

    let parts: Vec<String> = fields
        .iter()
        .map(|(name, value)| format!("{}: {}", quote_key(name), sample_value(value)))
        .collect();

    format!("{{ {} }}", parts.join(", "))
}

fn sample_value(sample: &Sample) -> String {
    match sample {
        Sample::Bool(true) => "true".to_string(),
        Sample::Bool(false) => "false".to_string(),
        Sample::Number(text) => text.clone(),
        Sample::Text(text) => json_string(text),
        Sample::List => "[]".to_string(),
        Sample::Map => "{}".to_string(),
        Sample::Null => "null".to_string(),
    }
}
