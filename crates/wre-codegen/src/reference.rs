use std::fmt::Write as _;

use serde_json::Value;
use wre_client::shape::{Field, Shape};
use wre_client::spec::OpSpec;

use crate::{Plan, summary_line};

pub(crate) struct Words {
    pub truth: &'static str,
    pub falsehood: &'static str,
}

pub(crate) struct Style<'a> {
    pub type_name: &'a dyn Fn(&Shape) -> String,
    pub signature: &'a dyn Fn(&OpSpec) -> String,
    pub words: Words,
}

pub(crate) enum Sample {
    Bool(bool),
    Number(String),
    Text(String),
    List,
    Map,
    Null,
}

pub(crate) fn notes_section(plan: &Plan) -> String {
    let notes = plan.client.notes.trim();
    if notes.is_empty() {
        return String::new();
    }

    format!("{notes}\n\n")
}

pub(crate) fn primary_op<'a>(plan: &'a Plan, takes_params: &dyn Fn(&Shape) -> bool) -> Option<&'a OpSpec> {
    if let Some(name) = &plan.client.primary {
        if let Some(found) = plan.client.ops.iter().find(|op| &op.name == name) {
            return Some(found);
        }
    }

    plan.client
        .ops
        .iter()
        .find(|op| takes_params(&op.params))
        .or_else(|| plan.client.ops.first())
}

pub(crate) fn config_section(plan: &Plan, style: &Style) -> String {
    let shape = resolve(plan, &plan.client.config);
    let Shape::Object { fields, .. } = shape else {
        return String::new();
    };

    if fields.is_empty() {
        return String::new();
    }

    let mut out = String::from(
        "## Configuration\n\nThese fields go in the object you pass to `open`. \
         The session keeps them for its whole life.\n\n",
    );

    out.push_str(&field_table(plan, style, fields));
    out.push('\n');
    out
}

pub(crate) fn operations_section(plan: &Plan, style: &Style) -> String {
    if plan.client.ops.is_empty() {
        return String::new();
    }

    let mut out = String::from("## Operations\n\n");

    for op in &plan.client.ops {
        let _ = writeln!(out, "### {}\n", (style.signature)(op));

        let summary = summary_line(&op.summary, "");
        if !summary.is_empty() {
            let _ = writeln!(out, "{summary}\n");
        }

        if op.deadline_ms > 0 {
            let _ = writeln!(
                out,
                "Server side deadline: {}. Pass a shorter one per call to cap it further.\n",
                duration(op.deadline_ms)
            );
        }

        if !op.streams.is_empty() {
            let names: Vec<String> =
                op.streams.iter().map(|event| format!("`{event}`")).collect();
            let _ = writeln!(out, "Streams: {}.\n", names.join(", "));
        }

        match resolve(plan, &op.params) {
            Shape::Object { fields, .. } if !fields.is_empty() => {
                out.push_str("Takes:\n\n");
                out.push_str(&field_table(plan, style, fields));
                out.push('\n');
            }
            _ => out.push_str("Takes no arguments.\n\n"),
        }

        match resolve(plan, &op.returns) {
            Shape::Object { fields, .. } if !fields.is_empty() => {
                out.push_str("Returns:\n\n");
                out.push_str(&returns_table(style, fields));
                out.push('\n');
            }
            other => {
                let _ = writeln!(out, "Returns `{}`.\n", escape((style.type_name)(other)));
            }
        }
    }

    out
}

pub(crate) fn events_table(plan: &Plan, style: &Style) -> String {
    if plan.client.events.is_empty() {
        return String::new();
    }

    let mut out = String::from("| Event | Data | Description |\n| --- | --- | --- |\n");

    for event in &plan.client.events {
        let _ = writeln!(
            out,
            "| `{}` | `{}` | {} |",
            event.name,
            escape((style.type_name)(&event.data)),
            cell(&summary_line(&event.summary, ""))
        );
    }

    out
}

pub(crate) fn example_params(shape: &Shape) -> Vec<(String, Sample)> {
    let Shape::Object { fields, .. } = shape else {
        return Vec::new();
    };

    let required: Vec<&Field> = fields.iter().filter(|field| field.required()).collect();

    let chosen: Vec<&Field> = if required.is_empty() {
        fields.iter().filter(|field| tells_something(field)).take(2).collect()
    } else {
        required.into_iter().take(3).collect()
    };

    chosen
        .into_iter()
        .map(|field| (field.name.clone(), sample_for(field)))
        .collect()
}

fn tells_something(field: &Field) -> bool {
    !matches!(sample_for(field), Sample::Map | Sample::List | Sample::Null)
}

fn sample_for(field: &Field) -> Sample {
    if let Some(default) = &field.default {
        if let Some(sample) = from_value(default) {
            return sample;
        }
    }

    let lowered = field.name.to_lowercase();
    if lowered.contains("url") || lowered.contains("endpoint") {
        return Sample::Text("https://example.com".to_string());
    }

    sample_of_shape(&field.shape)
}

fn sample_of_shape(shape: &Shape) -> Sample {
    match shape {
        Shape::Bool => Sample::Bool(true),
        Shape::Int | Shape::Float => Sample::Number("1".to_string()),
        Shape::Str => Sample::Text("...".to_string()),
        Shape::Bytes => Sample::Text(String::new()),
        Shape::List { .. } => Sample::List,
        Shape::Map { .. } | Shape::Object { .. } | Shape::Ref { .. } | Shape::Json => Sample::Map,
        Shape::Optional { of } => sample_of_shape(of),
        Shape::Enum { variants, .. } => variants
            .first()
            .map(|variant| Sample::Text(variant.clone()))
            .unwrap_or(Sample::Text(String::new())),
        Shape::Unit => Sample::Null,
    }
}

fn from_value(value: &Value) -> Option<Sample> {
    match value {
        Value::Bool(found) => Some(Sample::Bool(*found)),
        Value::Number(found) => Some(Sample::Number(found.to_string())),
        Value::String(found) => Some(Sample::Text(found.clone())),
        Value::Array(_) => Some(Sample::List),
        Value::Object(_) => Some(Sample::Map),
        Value::Null => None,
    }
}

fn field_table(plan: &Plan, style: &Style, fields: &[Field]) -> String {
    let mut out = String::from("| Field | Type | Default | Description |\n| --- | --- | --- | --- |\n");

    for field in fields {
        let _ = writeln!(
            out,
            "| `{}` | `{}` | {} | {} |",
            field.name,
            escape((style.type_name)(&field.shape)),
            default_cell(style, field),
            cell(&describe(plan, field))
        );
    }

    out
}

fn returns_table(style: &Style, fields: &[Field]) -> String {
    let mut out = String::from("| Field | Type | Description |\n| --- | --- | --- |\n");

    for field in fields {
        let _ = writeln!(
            out,
            "| `{}` | `{}` | {} |",
            field.name,
            escape((style.type_name)(&field.shape)),
            cell(&summary_line(&field.summary, ""))
        );
    }

    out
}

fn describe(plan: &Plan, field: &Field) -> String {
    let summary = summary_line(&field.summary, "");
    if !summary.is_empty() {
        return summary;
    }

    match resolve(plan, &field.shape) {
        Shape::Enum { variants, .. } => {
            let names: Vec<String> =
                variants.iter().map(|variant| format!("`{variant}`")).collect();
            format!("One of {}.", names.join(", "))
        }
        _ => String::new(),
    }
}

fn default_cell(style: &Style, field: &Field) -> String {
    match &field.default {
        Some(Value::Bool(found)) => {
            format!("`{}`", if *found { style.words.truth } else { style.words.falsehood })
        }
        Some(Value::Null) | None if field.required() => "required".to_string(),
        Some(value) => format!("`{value}`"),
        None => "optional".to_string(),
    }
}

fn cell(text: &str) -> String {
    if text.is_empty() { "-".to_string() } else { escape(text.to_string()) }
}

fn escape(text: String) -> String {
    text.replace('|', "\\|")
}

fn duration(ms: u64) -> String {
    if ms % 60_000 == 0 {
        format!("{} min", ms / 60_000)
    } else if ms % 1_000 == 0 {
        format!("{} s", ms / 1_000)
    } else {
        format!("{ms} ms")
    }
}

pub(crate) fn resolve<'a>(plan: &'a Plan, shape: &'a Shape) -> &'a Shape {
    match shape {
        Shape::Ref { name } => plan.client.types.get(name).unwrap_or(shape),
        _ => shape,
    }
}
