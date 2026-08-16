use std::fmt::Write as _;
use std::path::PathBuf;

use wre_client::shape::Shape;
use wre_client::spec::OpSpec;
use wre_core::error::Result;

use crate::names::{binary_name, pascal, snake, wheel_platform};
use crate::reference::{self, Sample, Style, Words};
use crate::{Language, Plan, copy_binary, json_string, summary_line, write};

pub fn emit(plan: &Plan) -> Result<Vec<PathBuf>> {
    let root = plan.root(Language::Python);
    let package = package_dir(plan);
    let mut files = Vec::new();

    files.push(write(&root.join("pyproject.toml"), &pyproject(plan))?);
    files.push(write(&root.join("setup.py"), &setup_py(plan))?);
    files.push(write(&root.join("build_wheels.sh"), &build_wheels(plan))?);
    files.push(write(&root.join("README.md"), &readme(plan))?);
    files.push(write(&root.join(&package).join("__init__.py"), &init_py(plan))?);
    files.push(write(&root.join(&package).join("_meta.py"), &meta_py(plan))?);
    files.push(write(&root.join(&package).join("types.py"), &types_py(plan))?);
    files.push(write(&root.join(&package).join("py.typed"), "")?);

    for entry in &plan.binaries.entries {
        let target = root
            .join("binaries")
            .join(&entry.triple)
            .join(binary_name(&entry.triple));
        files.push(copy_binary(&entry.path, &target)?);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = root.join("build_wheels.sh");
        if let Ok(metadata) = std::fs::metadata(&script) {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o755);
            let _ = std::fs::set_permissions(&script, permissions);
        }
    }

    Ok(files)
}

fn distribution(plan: &Plan) -> String {
    format!("{}client-{}", plan.config.python_prefix, plan.client.id.replace('_', "-"))
}

fn package_dir(plan: &Plan) -> String {
    format!("wre_client_{}", snake(&plan.client.id))
}

fn class_name(plan: &Plan) -> String {
    format!("{}Client", pascal(&plan.client.id))
}

fn pyproject(plan: &Plan) -> String {
    format!(
        r#"[build-system]
requires = ["setuptools>=68", "wheel"]
build-backend = "setuptools.build_meta"

[project]
name = "{name}"
version = "{version}"
description = "{description}"
requires-python = ">=3.9"
license = {{ text = "{license}" }}
dependencies = ["wre-runtime{runtime}"]

[project.urls]
Repository = "{repository}"

[tool.setuptools]
packages = ["{package}"]
include-package-data = true

[tool.setuptools.package-data]
"{package}" = ["py.typed", "bin/*/*"]
"#,
        name = distribution(plan),
        version = plan.config.version,
        description = summary_line(
            &plan.client.summary,
            &format!("Headless client for {}", plan.client.id)
        ),
        license = plan.config.license,
        runtime = plan.config.python_runtime,
        repository = plan.config.repository,
        package = package_dir(plan),
    )
}

fn setup_py(plan: &Plan) -> String {
    format!(
        r#"import os

from setuptools import setup

PLAT = os.environ.get("WRE_WHEEL_PLATFORM")

cmdclass = {{}}

if PLAT:
    try:
        from wheel.bdist_wheel import bdist_wheel
    except ImportError:
        from setuptools.command.bdist_wheel import bdist_wheel

    class PlatformWheel(bdist_wheel):
        def finalize_options(self):
            bdist_wheel.finalize_options(self)
            self.root_is_pure = False

        def get_tag(self):
            return ("py3", "none", PLAT)

    cmdclass["bdist_wheel"] = PlatformWheel

setup(
    packages=["{package}"],
    package_data={{"{package}": ["py.typed", "bin/*/*"]}},
    cmdclass=cmdclass,
)
"#,
        package = package_dir(plan),
    )
}

fn build_wheels(plan: &Plan) -> String {
    let mut rows = String::new();
    for entry in &plan.binaries.entries {
        let Some(tag) = wheel_platform(&entry.triple) else {
            continue;
        };
        let _ = writeln!(rows, "  \"{}:{}\"", entry.triple, tag);
    }

    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${{BASH_SOURCE[0]}}")" && pwd)"
package="{package}"

targets=(
{rows})

rm -rf "$here/dist"

for row in "${{targets[@]}}"; do
  triple="${{row%%:*}}"
  tag="${{row##*:}}"
  source="$here/binaries/$triple"

  if [ ! -d "$source" ]; then
    echo "skipping $triple, no binary in $source"
    continue
  fi

  rm -rf "$here/$package/bin" "$here/build" "$here"/*.egg-info
  mkdir -p "$here/$package/bin/$triple"
  cp "$source/"* "$here/$package/bin/$triple/"
  chmod +x "$here/$package/bin/$triple/"*

  echo "building wheel for $triple as $tag"
  WRE_WHEEL_PLATFORM="$tag" python3 -m build --wheel --outdir "$here/dist" "$here"
done

rm -rf "$here/$package/bin" "$here/build" "$here"/*.egg-info

limit=104857600
oversize=0
for wheel in "$here/dist/"*.whl; do
  bytes="$(wc -c < "$wheel")"
  printf '%8s MB  %s\n' "$((bytes / 1048576))" "$(basename "$wheel")"
  if [ "$bytes" -gt "$limit" ]; then
    echo "  over PyPI's 100 MB per file limit, it will be rejected" >&2
    oversize=1
  fi
done

if [ "$oversize" -ne 0 ]; then
  echo "refusing to hand oversized wheels to twine" >&2
  exit 1
fi

echo "wheels are in $here/dist"
"#,
        package = package_dir(plan),
    )
}

fn meta_py(plan: &Plan) -> String {
    let mut digests = String::new();
    for entry in &plan.binaries.entries {
        let _ = writeln!(
            digests,
            "    {}: {},",
            json_string(&entry.triple),
            json_string(&entry.sha256)
        );
    }

    format!(
        r#"from __future__ import annotations

from typing import Dict

TARGET = {target}
BUNDLE = {bundle}
CLIENT_VERSION = {client_version}
BINARY_VERSION = {binary_version}
SCHEMA_HASH = {schema_hash}

SHA256: Dict[str, str] = {{
{digests}}}
"#,
        target = json_string(&plan.client.id),
        bundle = json_string(&plan.bundle.bundle),
        client_version = json_string(&plan.client.version),
        binary_version = json_string(&plan.bundle.binary_version),
        schema_hash = json_string(&plan.schema_hash()),
    )
}

fn types_py(plan: &Plan) -> String {
    let mut out = String::new();
    out.push_str("from __future__ import annotations\n\n");
    out.push_str("from typing import Any, Dict, List, Literal, Optional, TypedDict\n\n");

    let mut names: Vec<String> = Vec::new();

    for (name, shape) in &plan.client.types {
        match shape {
            Shape::Enum { variants, .. } => {
                let joined: Vec<String> = variants.iter().map(|item| json_string(item)).collect();
                let _ = writeln!(out, "{name} = Literal[{}]\n", joined.join(", "));
                names.push(name.clone());
            }
            Shape::Object { fields, .. } => {
                let required: Vec<_> = fields.iter().filter(|entry| entry.required()).collect();
                let optional: Vec<_> = fields.iter().filter(|entry| !entry.required()).collect();

                if fields.is_empty() {
                    let _ = writeln!(out, "class {name}(TypedDict):\n    pass\n");
                } else if optional.is_empty() {
                    let _ = writeln!(out, "class {name}(TypedDict):");
                    for entry in &required {
                        let _ = writeln!(out, "    {}: {}", key(&entry.name), py_type(&entry.shape));
                    }
                    out.push('\n');
                } else if required.is_empty() {
                    let _ = writeln!(out, "class {name}(TypedDict, total=False):");
                    for entry in &optional {
                        let _ = writeln!(out, "    {}: {}", key(&entry.name), py_type(&entry.shape));
                    }
                    out.push('\n');
                } else {
                    let _ = writeln!(out, "class {name}Required(TypedDict):");
                    for entry in &required {
                        let _ = writeln!(out, "    {}: {}", key(&entry.name), py_type(&entry.shape));
                    }
                    out.push('\n');
                    let _ = writeln!(out, "class {name}({name}Required, total=False):");
                    for entry in &optional {
                        let _ = writeln!(out, "    {}: {}", key(&entry.name), py_type(&entry.shape));
                    }
                    out.push('\n');
                }

                names.push(name.clone());
            }
            _ => {}
        }
    }

    let exported: Vec<String> = names.iter().map(|name| json_string(name)).collect();
    let _ = writeln!(out, "__all__ = [{}]", exported.join(", "));

    out
}

fn key(name: &str) -> String {
    let reserved = [
        "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
        "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is",
        "lambda", "none", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
        "with", "yield",
    ];

    if reserved.contains(&name) { format!("{name}_") } else { name.to_string() }
}

fn py_type(shape: &Shape) -> String {
    match shape {
        Shape::Unit => "None".to_string(),
        Shape::Bool => "bool".to_string(),
        Shape::Int => "int".to_string(),
        Shape::Float => "float".to_string(),
        Shape::Str | Shape::Bytes => "str".to_string(),
        Shape::Json => "Any".to_string(),
        Shape::List { of } => format!("List[{}]", py_type(of)),
        Shape::Map { of } => format!("Dict[str, {}]", py_type(of)),
        Shape::Optional { of } => format!("Optional[{}]", py_type(of)),
        Shape::Enum { name, .. } | Shape::Object { name, .. } | Shape::Ref { name } => {
            format!("\"{name}\"")
        }
    }
}

fn init_py(plan: &Plan) -> String {
    let class = class_name(plan);
    let mut out = String::new();

    out.push_str("from __future__ import annotations\n\n");
    out.push_str("import os\n");
    out.push_str("from typing import Any, Callable, Mapping, Optional, Sequence\n\n");
    out.push_str(
        "from wre_runtime import Session, Sidecar, Unsupported, WreError, connect, current_triple, resolve_binary\n",
    );
    out.push_str("\nfrom ._meta import BINARY_VERSION, BUNDLE, CLIENT_VERSION, SCHEMA_HASH, SHA256, TARGET\n");
    out.push_str("from .types import *\n");
    out.push_str("from . import types as _types\n\n");

    out.push_str("def binary_path() -> str:\n");
    out.push_str("    if os.environ.get(\"WRE_BINARY\"):\n");
    out.push_str("        return resolve_binary()\n\n");
    out.push_str("    triple = current_triple()\n");
    out.push_str("    digest = SHA256.get(triple)\n");
    out.push_str("    if digest is None:\n");
    out.push_str(
        "        raise Unsupported(\"this package ships no binary for \" + triple + \", set WRE_BINARY to a wred you built\")\n\n",
    );
    out.push_str("    return resolve_binary(package_dir=os.path.dirname(__file__), sha256=digest)\n\n\n");

    let _ = writeln!(out, "class {class}:");
    out.push_str("    def __init__(self, sidecar: Sidecar, session: Session, owned: bool = False) -> None:\n");
    out.push_str("        self.sidecar = sidecar\n");
    out.push_str("        self.session = session\n");
    out.push_str("        self.owned = owned\n");
    out.push_str("        self._closed = False\n\n");

    out.push_str("    @classmethod\n");
    let _ = writeln!(
        out,
        "    def open(cls, config: Optional[{config}] = None, binary: Optional[str] = None, args: Sequence[str] = (), env: Optional[Mapping[str, str]] = None, cwd: Optional[str] = None, stderr: str = \"ignore\", on_event: Optional[Callable[[int, str, dict], None]] = None, check_schema: bool = True, startup_timeout: float = 30.0) -> \"{class}\":",
        config = annotation(&plan.client.config),
    );
    out.push_str("        sidecar = connect(\n");
    out.push_str("            binary=binary or binary_path(),\n");
    out.push_str("            args=args,\n");
    out.push_str("            env=env,\n");
    out.push_str("            cwd=cwd,\n");
    out.push_str("            stderr=stderr,\n");
    out.push_str("            on_event=on_event,\n");
    out.push_str("            expect_schema_hash=SCHEMA_HASH if check_schema else None,\n");
    out.push_str("            startup_timeout=startup_timeout,\n");
    out.push_str("        )\n\n");
    out.push_str("        try:\n");
    out.push_str("            session = sidecar.open(TARGET, dict(config or {}))\n");
    out.push_str("        except BaseException:\n");
    out.push_str("            sidecar.close()\n");
    out.push_str("            raise\n\n");
    out.push_str("        return cls(sidecar, session, owned=True)\n\n");

    out.push_str("    @classmethod\n");
    let _ = writeln!(
        out,
        "    def attach(cls, sidecar: Sidecar, config: Optional[{config}] = None) -> \"{class}\":",
        config = annotation(&plan.client.config),
    );
    out.push_str("        session = sidecar.open(TARGET, dict(config or {}))\n");
    out.push_str("        return cls(sidecar, session, owned=False)\n\n");

    for op in &plan.client.ops {
        emit_method_py(&mut out, op);
    }

    out.push_str("    def warmup(self) -> Any:\n");
    out.push_str("        return self.session.warmup()\n\n");
    out.push_str("    def health(self) -> Any:\n");
    out.push_str("        return self.session.health()\n\n");
    out.push_str("    def metrics(self) -> Any:\n");
    out.push_str("        return self.sidecar.metrics()\n\n");
    out.push_str("    def diagnose(self, write: bool = True) -> Any:\n");
    out.push_str("        return self.session.call(\"diag\", {\"write\": write, \"events\": True})\n\n");
    out.push_str("    def close(self) -> None:\n");
    out.push_str("        if self._closed:\n");
    out.push_str("            return\n");
    out.push_str("        self._closed = True\n");
    out.push_str("        try:\n");
    out.push_str("            self.session.close()\n");
    out.push_str("        finally:\n");
    out.push_str("            if self.owned:\n");
    out.push_str("                self.sidecar.close()\n\n");
    let _ = writeln!(out, "    def __enter__(self) -> \"{class}\":");
    out.push_str("        return self\n\n");
    out.push_str("    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:\n");
    out.push_str("        self.close()\n\n\n");

    let _ = writeln!(
        out,
        "def open_client(config: Optional[{config}] = None, **kwargs: Any) -> {class}:",
        config = annotation(&plan.client.config),
    );
    let _ = writeln!(out, "    return {class}.open(config, **kwargs)\n\n");

    let mut exported = vec![
        json_string(&class),
        json_string("open_client"),
        json_string("binary_path"),
        json_string("WreError"),
        json_string("TARGET"),
        json_string("BUNDLE"),
        json_string("CLIENT_VERSION"),
        json_string("BINARY_VERSION"),
        json_string("SCHEMA_HASH"),
    ];
    exported.push("*_types.__all__".to_string());

    let names = exported[..exported.len() - 1].join(", ");
    let _ = writeln!(out, "__all__ = [{names}] + list(_types.__all__)");

    out
}

fn emit_method_py(out: &mut String, op: &OpSpec) {
    let method = snake(&op.name);
    let returns = annotation(&op.returns);

    if has_params(&op.params) {
        let params = annotation(&op.params);
        let _ = writeln!(
            out,
            "    def {method}(self, params: {params}, deadline: Optional[float] = None, on_event: Optional[Callable[[int, str, dict], None]] = None) -> {returns}:"
        );
        let _ = writeln!(
            out,
            "        return self.session.call({}, dict(params), deadline=deadline, on_event=on_event)\n",
            json_string(&op.name)
        );
    } else {
        let _ = writeln!(
            out,
            "    def {method}(self, deadline: Optional[float] = None, on_event: Optional[Callable[[int, str, dict], None]] = None) -> {returns}:"
        );
        let _ = writeln!(
            out,
            "        return self.session.call({}, {{}}, deadline=deadline, on_event=on_event)\n",
            json_string(&op.name)
        );
    }
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

fn annotation(shape: &Shape) -> String {
    py_type(shape)
}

fn deadline_seconds(deadline_ms: u64) -> String {
    let ms = if deadline_ms > 0 { deadline_ms } else { 20_000 };
    let seconds = ms as f64 / 1000.0;
    if seconds.fract() == 0.0 {
        format!("{}", seconds as u64)
    } else {
        format!("{seconds}")
    }
}

fn style<'a>() -> Style<'a> {
    Style {
        type_name: &py_type,
        signature: &|op: &OpSpec| {
            if has_params(&op.params) {
                format!("`client.{}(params, deadline=None)`", snake(&op.name))
            } else {
                format!("`client.{}(deadline=None)`", snake(&op.name))
            }
        },
        words: Words { truth: "True", falsehood: "False" },
    }
}

fn readme(plan: &Plan) -> String {
    let class = class_name(plan);
    let distribution = distribution(plan);
    let package = package_dir(plan);
    let style = style();
    let notes = reference::notes_section(plan);
    let config = reference::config_section(plan, &style);
    let operations = reference::operations_section(plan, &style);

    let first = reference::primary_op(plan, &has_params);

    let call = match first {
        Some(op) if has_params(&op.params) => {
            format!("result = client.{}({})", snake(&op.name), sample(resolve(plan, &op.params)))
        }
        Some(op) => format!("result = client.{}()", snake(&op.name)),
        None => "result = client.health()".to_string(),
    };

    let deadline_call = match first {
        Some(op) if has_params(&op.params) => format!(
            "client.{}({}, deadline={})",
            snake(&op.name),
            sample(resolve(plan, &op.params)),
            deadline_seconds(op.deadline_ms)
        ),
        Some(op) => format!(
            "client.{}(deadline={})",
            snake(&op.name),
            deadline_seconds(op.deadline_ms)
        ),
        None => "client.health()".to_string(),
    };

    let streaming = plan.client.ops.iter().find(|op| !op.streams.is_empty());

    let events = match (streaming, plan.client.events.first()) {
        (Some(op), Some(event)) => format!(
            r#"## Events

`{op_name}` streams `{event_name}` while it runs. Pass `on_event` to `open` for every event on the session, or to a single call to scope it:

```python
with {class}.open(on_event=lambda call_id, event, data: print(event, data)) as client:
    client.{op_snake}({op_sample}, on_event=lambda call_id, event, data: print(event, data))
```

{event_table}
"#,
            op_name = op.name,
            op_snake = snake(&op.name),
            op_sample = sample(resolve(plan, &op.params)),
            event_name = event.name,
            event_table = reference::events_table(plan, &style),
        ),
        _ => String::new(),
    };

    format!(
        r#"# {distribution}

Generated client for the `{id}` target. It drives a `wred` sidecar over a pipe.

{summary}

## Install

```bash
pip install {distribution}
```

The wheel for your platform carries the binary. Set `WRE_BINARY` to an absolute path to run a local build instead.

## Use

```python
from {package} import {class}

with {class}.open() as client:
    {call}
    print(result)
```

The client owns one session, which owns the mounted realm. Keep it open and reuse it rather than opening one per call.

For asyncio, wrap the calls with `asyncio.to_thread`, or use `wre_runtime.aio.AsyncSidecar` and attach with `{class}.attach`.

{notes}{config}{operations}{events}## Deadlines

Every op takes `deadline`, in seconds. Past it the call fails with `wre_runtime.Timeout` and the work stops inside the sidecar.

```python
{deadline_call}
```

## Errors

Every failure raises a subclass of `wre_runtime.WreError`: `BadInput`, `Unsupported`, `TargetDrift`, `Blocked`, `Timeout`, `Cancelled`, `ResourceError`, `ProtocolError`, `InternalError`. Catch the class or branch on `error.kind`, never on the message. `error.retryable` says whether repeating the call is worth it.

## Sidecar output and diagnostics

The sidecar logs to its own stderr, which is discarded by default. Pass `stderr="inherit"` to `open`, or set `WRE_STDERR=inherit`, to see it.

A failing call writes a diagnostic report and puts its path in `error.detail["diagnostics"]`. `WRE_DIAG=always` records every call, `WRE_DIAG=off` records none, and `client.diagnose(True)` writes one on demand. Send that file with a bug report.

## Pinned build

- bundle `{bundle}`
- binary version `{binary_version}`
- schema hash `{schema_hash}`

The schema hash is verified when the sidecar starts. A mismatch means this package and the installed binary disagree about the callable surface, and the connect call fails.

## Building the wheels

`build_wheels.sh` copies each binary from `binaries/<triple>` into the package and builds one platform tagged wheel per triple into `dist/`.
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
        .map(|(name, value)| format!("\"{name}\": {}", sample_value(value)))
        .collect();

    format!("{{{}}}", parts.join(", "))
}

fn sample_value(sample: &Sample) -> String {
    match sample {
        Sample::Bool(true) => "True".to_string(),
        Sample::Bool(false) => "False".to_string(),
        Sample::Number(text) => text.clone(),
        Sample::Text(text) => json_string(text),
        Sample::List => "[]".to_string(),
        Sample::Map => "{}".to_string(),
        Sample::Null => "None".to_string(),
    }
}
