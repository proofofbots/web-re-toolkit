use std::fmt::Write as _;
use std::path::PathBuf;

use wre_client::shape::Shape;
use wre_client::spec::OpSpec;
use wre_core::error::Result;

use crate::names::{screaming, words};
use crate::{Language, Plan, download_url, json_string, summary_line, write};

fn go_name(value: &str) -> String {
    const INITIALISMS: [&str; 24] = [
        "id", "url", "uri", "api", "http", "https", "json", "xml", "html", "sql", "ssh", "tls",
        "ttl", "uuid", "sha", "md5", "cpu", "os", "ip", "ua", "jwt", "dns", "cdn", "ok",
    ];

    let mut out = String::new();

    for word in words(value) {
        if INITIALISMS.contains(&word.as_str()) {
            out.push_str(&word.to_uppercase());
            continue;
        }

        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push_str(&first.to_uppercase().collect::<String>());
            out.push_str(chars.as_str());
        }
    }

    out
}

pub fn emit(plan: &Plan) -> Result<Vec<PathBuf>> {
    let root = plan.root(Language::Go);
    let mut files = Vec::new();

    files.push(write(&root.join("go.mod"), &go_mod(plan))?);
    files.push(write(&root.join("meta.go"), &meta_go(plan))?);
    files.push(write(&root.join("types.go"), &types_go(plan))?);
    files.push(write(&root.join("client.go"), &client_go(plan))?);
    files.push(write(&root.join("README.md"), &readme(plan))?);

    Ok(files)
}

fn package_name(plan: &Plan) -> String {
    format!("client{}", plan.client.id.replace(['-', '_', '.'], ""))
}

fn module_path(plan: &Plan) -> String {
    format!("{}/{}", plan.config.go_module.trim_end_matches('/'), plan.client.id)
}

fn runtime_module(plan: &Plan) -> String {
    plan.config
        .go_runtime
        .split_whitespace()
        .next()
        .unwrap_or("github.com/proofofbots/web-re-toolkit/packages/go/wre")
        .to_string()
}

fn go_mod(plan: &Plan) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "module {}\n", module_path(plan));
    out.push_str("go 1.21\n\n");
    let _ = writeln!(out, "require {}\n", plan.config.go_runtime);

    if let Some(path) = &plan.config.go_runtime_replace {
        let _ = writeln!(out, "replace {} => {}", runtime_module(plan), path);
    }

    out
}

fn meta_go(plan: &Plan) -> String {
    let mut digests = String::new();
    for entry in &plan.binaries.entries {
        let _ = writeln!(
            digests,
            "\t{}: {},",
            json_string(&entry.triple),
            json_string(&entry.sha256)
        );
    }

    format!(
        r#"package {package}

import (
	"fmt"

	"{runtime}"
)

const (
	Target        = {target}
	Bundle        = {bundle}
	ClientVersion = {client_version}
	BinaryVersion = {binary_version}
	SchemaHash    = {schema_hash}
	DownloadURL   = {download}
)

var sha256ByTriple = map[string]string{{
{digests}}}

func BinaryPath() (string, error) {{
	triple, err := wre.CurrentTriple()
	if err != nil {{
		return "", err
	}}

	digest, ok := sha256ByTriple[triple]
	if !ok {{
		return "", fmt.Errorf("this package ships no binary for %s, set WRE_BINARY to a wred you built", triple)
	}}

	return wre.ResolveBinary(wre.BinarySpec{{
		Version: BinaryVersion,
		Triple:  triple,
		SHA256:  digest,
		URL:     DownloadURL,
	}})
}}
"#,
        package = package_name(plan),
        runtime = runtime_module(plan),
        target = json_string(&plan.client.id),
        bundle = json_string(&plan.bundle.bundle),
        client_version = json_string(&plan.client.version),
        binary_version = json_string(&plan.bundle.binary_version),
        schema_hash = json_string(&plan.schema_hash()),
        download = json_string(&download_url(plan.config, "{triple}")),
    )
}

fn types_go(plan: &Plan) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "package {}\n", package_name(plan));

    if needs_json_import(plan) {
        out.push_str("import \"encoding/json\"\n\n");
    }

    for (name, shape) in &plan.client.types {
        match shape {
            Shape::Enum { variants, .. } => {
                let _ = writeln!(out, "type {name} string\n");
                out.push_str("const (\n");
                for variant in variants {
                    let _ = writeln!(
                        out,
                        "\t{name}{} {name} = {}",
                        go_name(variant),
                        json_string(variant)
                    );
                }
                out.push_str(")\n\n");
            }
            Shape::Object { fields, .. } => {
                let _ = writeln!(out, "type {name} struct {{");
                for entry in fields {
                    let tag = if entry.required() {
                        format!("`json:\"{}\"`", entry.name)
                    } else {
                        format!("`json:\"{},omitempty\"`", entry.name)
                    };
                    let _ = writeln!(
                        out,
                        "\t{} {} {tag}",
                        go_name(&entry.name),
                        go_type(&entry.shape, entry.required())
                    );
                }
                out.push_str("}\n\n");
            }
            _ => {}
        }
    }

    out
}

fn needs_json_import(plan: &Plan) -> bool {
    plan.client
        .types
        .values()
        .any(|shape| shape_uses_json(shape))
}

fn shape_uses_json(shape: &Shape) -> bool {
    match shape {
        Shape::Json => true,
        Shape::List { of } | Shape::Map { of } | Shape::Optional { of } => shape_uses_json(of),
        Shape::Object { fields, .. } => fields.iter().any(|entry| shape_uses_json(&entry.shape)),
        _ => false,
    }
}

fn go_type(shape: &Shape, required: bool) -> String {
    match shape {
        Shape::Optional { of } => go_type(of, false),
        Shape::Unit => "any".to_string(),
        Shape::Bool => pointer("bool", required),
        Shape::Int => pointer("int64", required),
        Shape::Float => pointer("float64", required),
        Shape::Str | Shape::Bytes => pointer("string", required),
        Shape::Json => "json.RawMessage".to_string(),
        Shape::List { of } => format!("[]{}", go_type(of, true)),
        Shape::Map { of } => format!("map[string]{}", go_type(of, true)),
        Shape::Enum { name, .. } | Shape::Object { name, .. } | Shape::Ref { name } => {
            pointer(name, required)
        }
    }
}

fn pointer(name: &str, required: bool) -> String {
    if required { name.to_string() } else { format!("*{name}") }
}

fn return_type(shape: &Shape) -> String {
    match shape {
        Shape::Unit => "any".to_string(),
        Shape::Json => "json.RawMessage".to_string(),
        other => go_type(other, true),
    }
}

fn zero_value(shape: &Shape) -> String {
    match shape {
        Shape::Bool => "false".to_string(),
        Shape::Int => "0".to_string(),
        Shape::Float => "0".to_string(),
        Shape::Str | Shape::Bytes => "\"\"".to_string(),
        Shape::Json => "nil".to_string(),
        Shape::List { .. } | Shape::Map { .. } | Shape::Optional { .. } | Shape::Unit => {
            "nil".to_string()
        }
        Shape::Enum { name, .. } => format!("{name}(\"\")"),
        Shape::Object { name, .. } | Shape::Ref { name } => format!("{name}{{}}"),
    }
}

fn client_go(plan: &Plan) -> String {
    let package = package_name(plan);
    let runtime = runtime_module(plan);
    let config_type = go_type(&plan.client.config, true);
    let mut out = String::new();

    let _ = writeln!(out, "package {package}\n");
    out.push_str("import (\n");
    out.push_str("\t\"context\"\n");
    out.push_str("\t\"encoding/json\"\n");
    out.push_str("\t\"io\"\n");
    out.push_str("\t\"time\"\n\n");
    let _ = writeln!(out, "\t\"{runtime}\"\n)");
    out.push('\n');

    out.push_str("type OpenOptions struct {\n");
    out.push_str("\tBinary          string\n");
    out.push_str("\tArgs            []string\n");
    out.push_str("\tEnv             []string\n");
    out.push_str("\tDir             string\n");
    out.push_str("\tStderr          io.Writer\n");
    out.push_str("\tOnEvent         func(id uint64, event string, data json.RawMessage)\n");
    out.push_str("\tSkipSchemaCheck bool\n");
    out.push_str("\tStartupTimeout  time.Duration\n");
    out.push_str("\tDiag            map[string]any\n");
    out.push_str("}\n\n");

    out.push_str("type Client struct {\n");
    out.push_str("\tSidecar *wre.Sidecar\n");
    out.push_str("\tSession *wre.Session\n");
    out.push_str("\towned   bool\n");
    out.push_str("\tclosed  bool\n");
    out.push_str("}\n\n");

    let _ = writeln!(
        out,
        "func Open(ctx context.Context, config *{config_type}, opts OpenOptions) (*Client, error) {{"
    );
    out.push_str("\tbinary := opts.Binary\n");
    out.push_str("\tif binary == \"\" {\n");
    out.push_str("\t\tresolved, err := BinaryPath()\n");
    out.push_str("\t\tif err != nil {\n");
    out.push_str("\t\t\treturn nil, err\n");
    out.push_str("\t\t}\n");
    out.push_str("\t\tbinary = resolved\n");
    out.push_str("\t}\n\n");
    out.push_str("\texpect := SchemaHash\n");
    out.push_str("\tif opts.SkipSchemaCheck {\n");
    out.push_str("\t\texpect = \"\"\n");
    out.push_str("\t}\n\n");
    out.push_str("\tsidecar, err := wre.Connect(ctx, wre.Options{\n");
    out.push_str("\t\tBinary:           binary,\n");
    out.push_str("\t\tArgs:             opts.Args,\n");
    out.push_str("\t\tEnv:              opts.Env,\n");
    out.push_str("\t\tDir:              opts.Dir,\n");
    out.push_str("\t\tStderr:           opts.Stderr,\n");
    out.push_str("\t\tOnEvent:          opts.OnEvent,\n");
    out.push_str("\t\tExpectSchemaHash: expect,\n");
    out.push_str("\t\tStartupTimeout:   opts.StartupTimeout,\n");
    out.push_str("\t})\n");
    out.push_str("\tif err != nil {\n");
    out.push_str("\t\treturn nil, err\n");
    out.push_str("\t}\n\n");
    out.push_str("\tsession, err := sidecar.Open(ctx, Target, config)\n");
    out.push_str("\tif err != nil {\n");
    out.push_str("\t\tsidecar.Close()\n");
    out.push_str("\t\treturn nil, err\n");
    out.push_str("\t}\n\n");
    out.push_str("\treturn &Client{Sidecar: sidecar, Session: session, owned: true}, nil\n");
    out.push_str("}\n\n");

    let _ = writeln!(
        out,
        "func Attach(ctx context.Context, sidecar *wre.Sidecar, config *{config_type}) (*Client, error) {{"
    );
    out.push_str("\tsession, err := sidecar.Open(ctx, Target, config)\n");
    out.push_str("\tif err != nil {\n");
    out.push_str("\t\treturn nil, err\n");
    out.push_str("\t}\n\n");
    out.push_str("\treturn &Client{Sidecar: sidecar, Session: session, owned: false}, nil\n");
    out.push_str("}\n\n");

    for op in &plan.client.ops {
        emit_method_go(&mut out, op);
    }

    out.push_str("func (c *Client) Warmup(ctx context.Context) error {\n");
    out.push_str("\treturn c.Session.Warmup(ctx)\n");
    out.push_str("}\n\n");

    out.push_str("func (c *Client) Health(ctx context.Context) (wre.Health, error) {\n");
    out.push_str("\treturn c.Session.Health(ctx)\n");
    out.push_str("}\n\n");

    out.push_str("func (c *Client) Diagnose(ctx context.Context, write bool) (json.RawMessage, error) {\n");
    out.push_str("\treturn c.Session.CallRaw(ctx, \"diag\", map[string]any{\"write\": write, \"events\": true})\n");
    out.push_str("}\n\n");

    out.push_str("func (c *Client) Metrics(ctx context.Context) (json.RawMessage, error) {\n");
    out.push_str("\treturn c.Sidecar.Metrics(ctx)\n");
    out.push_str("}\n\n");

    out.push_str("func (c *Client) Close(ctx context.Context) error {\n");
    out.push_str("\tif c.closed {\n");
    out.push_str("\t\treturn nil\n");
    out.push_str("\t}\n");
    out.push_str("\tc.closed = true\n\n");
    out.push_str("\terr := c.Session.Close(ctx)\n");
    out.push_str("\tif c.owned {\n");
    out.push_str("\t\tif closeErr := c.Sidecar.Close(); err == nil {\n");
    out.push_str("\t\t\terr = closeErr\n");
    out.push_str("\t\t}\n");
    out.push_str("\t}\n\n");
    out.push_str("\treturn err\n");
    out.push_str("}\n");

    out
}

fn emit_method_go(out: &mut String, op: &OpSpec) {
    let method = go_name(&op.name);
    let returns = return_type(&op.returns);
    let zero = zero_value(&op.returns);
    let takes_params = has_params(&op.params);

    if takes_params {
        let params = go_type(&op.params, true);
        let _ = writeln!(
            out,
            "func (c *Client) {method}(ctx context.Context, params {params}) ({returns}, error) {{"
        );
    } else {
        let _ = writeln!(
            out,
            "func (c *Client) {method}(ctx context.Context) ({returns}, error) {{"
        );
    }

    let _ = writeln!(out, "\tvar out {returns}");

    let argument = if takes_params { "params" } else { "map[string]any{}" };
    let _ = writeln!(
        out,
        "\tif err := c.Session.Call(ctx, {}, {argument}, &out); err != nil {{",
        json_string(&op.name)
    );
    let _ = writeln!(out, "\t\treturn {zero}, err");
    out.push_str("\t}\n\n");
    out.push_str("\treturn out, nil\n");
    out.push_str("}\n\n");
}

fn has_params(shape: &Shape) -> bool {
    match shape {
        Shape::Object { fields, .. } => !fields.is_empty(),
        Shape::Unit => false,
        _ => true,
    }
}

fn readme(plan: &Plan) -> String {
    let package = package_name(plan);
    let module = module_path(plan);

    let first = plan
        .client
        .ops
        .iter()
        .find(|op| has_params(&op.params))
        .or_else(|| plan.client.ops.first());

    let call = match first {
        Some(op) if has_params(&op.params) => format!(
            "result, err := client.{}(ctx, {}{{}})",
            go_name(&op.name),
            go_type(&op.params, true)
        ),
        Some(op) => format!("result, err := client.{}(ctx)", go_name(&op.name)),
        None => "result, err := client.Health(ctx)".to_string(),
    };

    format!(
        r#"# {module}

Generated client for the `{id}` target. It drives a `wred` sidecar over a pipe.

{summary}

## Install

```bash
go get {module}
```

The binary is downloaded on first use into the cache directory and verified against a pinned sha256. `WRE_BINARY` points at a local build instead, and `WRE_CACHE_DIR` moves the cache.

## Use

```go
ctx := context.Background()

client, err := {package}.Open(ctx, nil, {package}.OpenOptions{{}})
if err != nil {{
	log.Fatal(err)
}}
defer client.Close(ctx)

{call}
if err != nil {{
	log.Fatal(err)
}}
fmt.Println(result)
```

Check error kinds with `wre.IsKind(err, wre.KindTargetDrift)`.

## Pinned build

- bundle `{bundle}`
- binary version `{binary_version}`
- schema hash `{schema_hash}`

The schema hash is verified during `Connect`. A mismatch means this package and the installed binary disagree about the callable surface.

## Diagnostics

`client.Diagnose(ctx, true)` writes one json report with the session's call history, the host facts and the target specific debug section, and returns its path.
"#,
        id = plan.client.id,
        summary = summary_line(&plan.client.summary, "No summary was declared for this target."),
        bundle = plan.bundle.bundle,
        binary_version = plan.bundle.binary_version,
        schema_hash = plan.schema_hash(),
    )
}

pub fn screaming_name(value: &str) -> String {
    screaming(value)
}
