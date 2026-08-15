pub mod capture;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use wre_core::error::{Error, Result};
use wre_live::realm::Realm;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureOptions {
    pub depth: usize,
    pub max_props: usize,
    pub max_string: usize,
    #[serde(default)]
    pub roots: Vec<String>,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            depth: 4,
            max_props: 400,
            max_string: 4096,
            roots: [
                "window",
                "navigator",
                "screen",
                "location",
                "history",
                "performance",
                "crypto",
                "Intl",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        }
    }
}

pub fn capture_script(options: &CaptureOptions) -> Result<String> {
    let config = serde_json::to_string(&json!({
        "depth": options.depth,
        "maxProps": options.max_props,
        "maxString": options.max_string,
        "roots": options.roots,
    }))
    .map_err(|error| Error::msg(format!("capture options did not serialise: {error}")))?;

    Ok(capture::CAPTURE.replace("__WRE_SNAPSHOT_OPTIONS__", &config))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub captured_at: f64,
    #[serde(default)]
    pub href: String,
    #[serde(default)]
    pub user_agent: String,
    #[serde(default)]
    pub roots: BTreeMap<String, usize>,
    #[serde(default)]
    pub objects: Vec<ObjectRecord>,
    #[serde(default)]
    pub globals: Vec<String>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ObjectRecord {
    pub id: usize,
    #[serde(default)]
    pub cls: String,
    #[serde(default)]
    pub callable: bool,
    #[serde(default)]
    pub array: bool,
    #[serde(default)]
    pub length: usize,
    #[serde(default, rename = "fnName")]
    pub fn_name: Option<String>,
    #[serde(default, rename = "fnLength")]
    pub fn_length: Option<usize>,
    #[serde(default)]
    pub native: bool,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
    #[serde(default)]
    pub getters: Vec<String>,
    #[serde(default)]
    pub throwing: Vec<String>,
    #[serde(default)]
    pub proto: Option<usize>,
}

impl Snapshot {
    pub fn parse(value: &Value) -> Result<Self> {
        let mut snapshot = Snapshot {
            version: value.get("version").and_then(Value::as_u64).unwrap_or(1) as u32,
            captured_at: value.get("capturedAt").and_then(Value::as_f64).unwrap_or(0.0),
            href: value
                .get("href")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            user_agent: value
                .get("userAgent")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            truncated: value
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            ..Snapshot::default()
        };

        if let Some(roots) = value.get("roots").and_then(Value::as_object) {
            for (name, id) in roots {
                if let Some(id) = id.as_u64() {
                    snapshot.roots.insert(name.clone(), id as usize);
                }
            }
        }

        if let Some(objects) = value.get("objects").and_then(Value::as_array) {
            for object in objects {
                match serde_json::from_value::<ObjectRecord>(object.clone()) {
                    Ok(record) => snapshot.objects.push(record),
                    Err(error) => {
                        return Err(Error::msg(format!("snapshot object did not parse: {error}")));
                    }
                }
            }
        }

        if let Some(globals) = value.get("globals").and_then(Value::as_array) {
            snapshot.globals = globals
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect();
        }

        Ok(snapshot)
    }

    pub fn to_value(&self) -> Result<Value> {
        let mut roots = serde_json::Map::new();
        for (name, id) in &self.roots {
            roots.insert(name.clone(), json!(id));
        }

        let objects: Vec<Value> = self
            .objects
            .iter()
            .map(|record| {
                let mut entry = serde_json::Map::new();
                entry.insert("id".into(), json!(record.id));
                entry.insert("cls".into(), json!(record.cls));
                entry.insert("callable".into(), json!(record.callable));
                if record.array {
                    entry.insert("array".into(), json!(true));
                    entry.insert("length".into(), json!(record.length));
                }
                if let Some(name) = &record.fn_name {
                    entry.insert("fnName".into(), json!(name));
                }
                if let Some(length) = record.fn_length {
                    entry.insert("fnLength".into(), json!(length));
                }
                entry.insert("native".into(), json!(record.native));
                entry.insert(
                    "props".into(),
                    Value::Object(record.props.clone().into_iter().collect()),
                );
                entry.insert("getters".into(), json!(record.getters));
                entry.insert("throwing".into(), json!(record.throwing));
                entry.insert("proto".into(), json!(record.proto));
                Value::Object(entry)
            })
            .collect();

        Ok(json!({
            "version": self.version,
            "capturedAt": self.captured_at,
            "href": self.href,
            "userAgent": self.user_agent,
            "roots": Value::Object(roots),
            "objects": objects,
            "globals": self.globals,
            "truncated": self.truncated,
        }))
    }

    pub fn object(&self, id: usize) -> Option<&ObjectRecord> {
        self.objects.iter().find(|record| record.id == id)
    }

    pub fn root(&self, name: &str) -> Option<&ObjectRecord> {
        self.roots.get(name).and_then(|id| self.object(*id))
    }

    pub fn lookup(&self, path: &str) -> Option<&Value> {
        let mut parts = path.split('.');
        let root = parts.next()?;
        let mut record = self.root(root)?;
        let mut last: Option<&Value> = None;

        for part in parts {
            let value = record.props.get(part)?;
            last = Some(value);

            if let Some(id) = value.get("id").and_then(Value::as_u64) {
                if value.get("k").and_then(Value::as_str) == Some("ref") {
                    record = self.object(id as usize)?;
                    continue;
                }
            }
        }

        last
    }

    pub fn read(&self, path: &str) -> Option<Value> {
        let encoded = self.lookup(path)?;
        decode_scalar(encoded)
    }

    pub fn function_count(&self) -> usize {
        self.objects.iter().filter(|record| record.callable).count()
    }

    pub fn getter_count(&self) -> usize {
        self.objects.iter().map(|record| record.getters.len()).sum()
    }
}

pub fn decode_scalar(value: &Value) -> Option<Value> {
    match value.get("k").and_then(Value::as_str)? {
        "null" => Some(Value::Null),
        "undef" | "deep" => None,
        "bool" | "num" | "str" => value.get("v").cloned(),
        "nan" => Some(json!("NaN")),
        "inf" => Some(json!(if value.get("v").and_then(Value::as_i64).unwrap_or(1) > 0 {
            "Infinity"
        } else {
            "-Infinity"
        })),
        "bigint" | "symbol" => value.get("v").cloned(),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterializeOptions {
    #[serde(default)]
    pub record_calls: bool,
    #[serde(default)]
    pub bridge: Option<String>,
}

impl Default for MaterializeOptions {
    fn default() -> Self {
        Self { record_calls: true, bridge: None }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MaterializeReport {
    #[serde(default)]
    pub roots: Vec<String>,
    #[serde(default)]
    pub objects: usize,
    #[serde(default)]
    pub missing: Vec<String>,
}

pub fn materialize(
    realm: &mut Realm,
    snapshot: &Snapshot,
    options: &MaterializeOptions,
) -> Result<MaterializeReport> {
    let value = snapshot.to_value()?;
    let payload = serde_json::to_string(&value)
        .map_err(|error| Error::msg(format!("snapshot did not serialise: {error}")))?;

    let bridge = match &options.bridge {
        Some(name) => name.clone(),
        None => "null".to_string(),
    };

    let config = format!(
        "{{ recordCalls: {}, bridge: {bridge} }}",
        options.record_calls
    );

    let script = capture::MATERIALIZE
        .replace("__WRE_SNAPSHOT__", &payload)
        .replace("__WRE_MATERIALIZE_OPTIONS__", &config);

    let report = realm.eval(&script, "wre:materialize")?;

    serde_json::from_value(report)
        .map_err(|error| Error::msg(format!("materialise report did not parse: {error}")))
}

pub fn synthetic_snapshot() -> Snapshot {
    let mut snapshot = Snapshot {
        version: 1,
        href: "https://example.test/".to_string(),
        user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36".to_string(),
        ..Snapshot::default()
    };

    let navigator = ObjectRecord {
        id: 0,
        cls: "Navigator".to_string(),
        props: [
            ("userAgent".to_string(), json!({ "k": "str", "v": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36" })),
            ("platform".to_string(), json!({ "k": "str", "v": "MacIntel" })),
            ("hardwareConcurrency".to_string(), json!({ "k": "num", "v": 8 })),
            ("webdriver".to_string(), json!({ "k": "bool", "v": false })),
            ("languages".to_string(), json!({ "k": "ref", "id": 1 })),
        ]
        .into_iter()
        .collect(),
        getters: vec!["userAgent".to_string(), "platform".to_string()],
        ..ObjectRecord::default()
    };

    let languages = ObjectRecord {
        id: 1,
        cls: "Array".to_string(),
        array: true,
        length: 2,
        props: [
            ("0".to_string(), json!({ "k": "str", "v": "en-US" })),
            ("1".to_string(), json!({ "k": "str", "v": "en" })),
            ("length".to_string(), json!({ "k": "num", "v": 2 })),
        ]
        .into_iter()
        .collect(),
        ..ObjectRecord::default()
    };

    let screen = ObjectRecord {
        id: 2,
        cls: "Screen".to_string(),
        props: [
            ("width".to_string(), json!({ "k": "num", "v": 1512 })),
            ("height".to_string(), json!({ "k": "num", "v": 982 })),
            ("colorDepth".to_string(), json!({ "k": "num", "v": 24 })),
        ]
        .into_iter()
        .collect(),
        ..ObjectRecord::default()
    };

    snapshot.objects = vec![navigator, languages, screen];
    snapshot.roots.insert("navigator".to_string(), 0);
    snapshot.roots.insert("screen".to_string(), 2);
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_script_inlines_options() {
        let script = capture_script(&CaptureOptions::default()).unwrap();
        assert!(!script.contains("__WRE_SNAPSHOT_OPTIONS__"));
        assert!(script.contains("\"depth\":4"));
        assert!(script.contains("navigator"));
    }

    #[test]
    fn snapshot_round_trips_through_json() {
        let snapshot = synthetic_snapshot();
        let value = snapshot.to_value().unwrap();
        let back = Snapshot::parse(&value).unwrap();

        assert_eq!(back.objects.len(), snapshot.objects.len());
        assert_eq!(back.roots, snapshot.roots);
        assert_eq!(back.user_agent, snapshot.user_agent);
    }

    #[test]
    fn reads_scalars_by_path() {
        let snapshot = synthetic_snapshot();
        assert_eq!(snapshot.read("navigator.platform"), Some(json!("MacIntel")));
        assert_eq!(snapshot.read("navigator.hardwareConcurrency"), Some(json!(8)));
        assert_eq!(snapshot.read("screen.width"), Some(json!(1512)));
        assert_eq!(snapshot.read("navigator.nothing"), None);
    }

    #[test]
    fn counts_functions_and_getters() {
        let snapshot = synthetic_snapshot();
        assert_eq!(snapshot.function_count(), 0);
        assert_eq!(snapshot.getter_count(), 2);
    }
}
