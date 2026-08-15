use std::path::PathBuf;

use serde_json::json;

use wre_core::error::{Error, Result};
use wre_report::table::Table;
use wre_wire::codec::{
    Base64JsonCodec, Codec, DeflateJsonCodec, JsonCodec, XorCodec, XorInner, verify_roundtrip,
};
use wre_wire::payload::{Patch, Payload, diff, forge};
use wre_wire::schema::infer;

use crate::args::WireCommand;
use crate::{Context, read_bytes, read_json, write_text};

pub fn run(context: &Context, command: WireCommand) -> Result<()> {
    match command {
        WireCommand::Open { input, codec, key, out } => open(context, &input, &codec, key, out),
        WireCommand::Seal { input, codec, key, out } => seal(context, &input, &codec, key, out),
        WireCommand::Roundtrip { input, codec, key } => roundtrip(context, &input, &codec, key),
        WireCommand::Diff { left, right } => diff_cmd(context, &left, &right),
        WireCommand::Forge { donor, set, out } => forge_cmd(context, &donor, &set, out),
        WireCommand::Schema { inputs, out } => schema_cmd(context, &inputs, out),
    }
}

pub fn build_codec(name: &str, key: Option<String>) -> Result<Box<dyn Codec>> {
    Ok(match name {
        "json" => Box::new(JsonCodec),
        "base64" | "base64+json" => Box::new(Base64JsonCodec),
        "deflate-raw" | "deflate-raw+json" => Box::new(DeflateJsonCodec::raw()),
        "deflate" | "deflate+json" => Box::new(DeflateJsonCodec::zlib()),
        "xor" => {
            let hex = key.ok_or_else(|| Error::msg("the xor codec needs --key as hex"))?;
            let bytes = hex::decode(hex.trim())
                .map_err(|error| Error::msg(format!("--key is not hex: {error}")))?;
            Box::new(XorCodec::new(bytes, XorInner::Json)?)
        }
        other => {
            return Err(Error::msg(format!(
                "unknown codec {other}, try json, base64, deflate-raw, deflate or xor"
            )));
        }
    })
}

fn open(
    context: &Context,
    input: &std::path::Path,
    codec: &str,
    key: Option<String>,
    out: Option<PathBuf>,
) -> Result<()> {
    let bytes = read_bytes(input)?;
    let mut codec = build_codec(codec, key)?;
    let value = codec.open(&bytes)?;
    let text = serde_json::to_string_pretty(&value).unwrap_or_default();

    match out {
        Some(path) => {
            write_text(&path, &format!("{text}\n"))?;
            context.emit(
                &json!({ "output": path.display().to_string(), "leaves": Payload::new(value).leaf_count() }),
                &format!("wrote {}\n", path.display()),
            );
        }
        None => context.emit(&value, &text),
    }

    Ok(())
}

fn seal(
    context: &Context,
    input: &std::path::Path,
    codec: &str,
    key: Option<String>,
    out: Option<PathBuf>,
) -> Result<()> {
    let value = read_json(input)?;
    let mut codec = build_codec(codec, key)?;
    let bytes = codec.seal(&value)?;

    match out {
        Some(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(wre_core::error::io(parent))?;
            }
            std::fs::write(&path, &bytes).map_err(wre_core::error::io(&path))?;
            context.emit(
                &json!({ "output": path.display().to_string(), "bytes": bytes.len() }),
                &format!("wrote {} ({} bytes)\n", path.display(), bytes.len()),
            );
        }
        None => {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            context.emit(&json!({ "bytes": bytes.len() }), &text);
        }
    }

    Ok(())
}

fn roundtrip(
    context: &Context,
    input: &std::path::Path,
    codec: &str,
    key: Option<String>,
) -> Result<()> {
    let bytes = read_bytes(input)?;
    let mut codec = build_codec(codec, key)?;
    let report = verify_roundtrip(codec.as_mut(), &bytes);

    let plain = format!(
        "{}: opened {}, resealed {}, identical {}{}\n",
        report.codec,
        report.opened,
        report.resealed,
        report.identical,
        report
            .note
            .as_ref()
            .map(|note| format!(" ({note})"))
            .unwrap_or_default()
    );

    context.emit(&json!(report), &plain);

    if !report.ok() {
        return Err(Error::msg("the round trip did not reproduce the original bytes"));
    }

    Ok(())
}

fn diff_cmd(context: &Context, left: &std::path::Path, right: &std::path::Path) -> Result<()> {
    let before = read_json(left)?;
    let after = read_json(right)?;
    let changes = diff(&before, &after);

    let mut table = Table::new(&["address", "change", "before", "after"]);
    for entry in &changes {
        table.push(vec![
            format!("`{}`", entry.address),
            format!("{:?}", entry.change).to_lowercase(),
            entry
                .left
                .as_ref()
                .map(|value| value.to_string())
                .unwrap_or_default(),
            entry
                .right
                .as_ref()
                .map(|value| value.to_string())
                .unwrap_or_default(),
        ]);
    }

    let plain = if changes.is_empty() {
        "the two payloads carry the same leaves\n".to_string()
    } else {
        format!("{} addresses moved\n{}", changes.len(), table.render())
    };

    context.emit(&json!(changes), &plain);
    Ok(())
}

fn forge_cmd(
    context: &Context,
    donor: &std::path::Path,
    set: &[String],
    out: Option<PathBuf>,
) -> Result<()> {
    let payload = Payload::new(read_json(donor)?);

    let mut patches = Vec::with_capacity(set.len());
    for spec in set {
        patches.push(Patch::parse(spec)?);
    }

    let (forged, report) = forge(&payload, &patches)?;
    let text = serde_json::to_string_pretty(&forged.value).unwrap_or_default();

    match out {
        Some(path) => {
            write_text(&path, &format!("{text}\n"))?;
            context.emit(
                &json!(report),
                &format!(
                    "wrote {}\n  {} patches applied, {} addresses overwritten, {} kept from the donor\n",
                    path.display(),
                    report.applied,
                    report.overwritten,
                    report.from_donor
                ),
            );
        }
        None => context.emit(&forged.value, &text),
    }

    Ok(())
}

fn schema_cmd(context: &Context, inputs: &[PathBuf], out: Option<PathBuf>) -> Result<()> {
    if inputs.is_empty() {
        return Err(Error::msg("give at least one payload"));
    }

    let mut samples = Vec::with_capacity(inputs.len());
    for path in inputs {
        samples.push(read_json(path)?);
    }

    let schema = infer(&samples);

    let mut table = Table::new(&["address", "volatility", "present", "distinct", "sample"]);
    for field in &schema.fields {
        table.push(vec![
            format!("`{}`", field.address),
            format!("{:?}", field.volatility).to_lowercase(),
            format!("{}/{}", field.present_in, schema.samples),
            field.distinct.to_string(),
            field
                .samples
                .first()
                .map(|value| value.to_string())
                .unwrap_or_default(),
        ]);
    }

    if let Some(path) = &out {
        write_text(
            path,
            &format!("{}\n", serde_json::to_string_pretty(&schema).unwrap_or_default()),
        )?;
    }

    let plain = format!(
        "{} fields across {} samples ({} constant, {} volatile)\n{}",
        schema.fields.len(),
        schema.samples,
        schema.stable().len(),
        schema.volatile().len(),
        table.render()
    );

    context.emit(&json!(schema), &plain);
    Ok(())
}
