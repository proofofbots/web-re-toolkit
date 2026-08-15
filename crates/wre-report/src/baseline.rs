use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use wre_core::error::{Error, Result, io, json};

use crate::table::Table;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub name: String,
    pub saved_at: String,
    pub map: Value,
    #[serde(default)]
    pub notes: BTreeMap<String, String>,
}

impl Baseline {
    pub fn new(name: &str, map: Value) -> Self {
        Self {
            name: name.to_string(),
            saved_at: chrono::Utc::now().to_rfc3339(),
            map,
            notes: BTreeMap::new(),
        }
    }

    pub fn save(&self, dir: &Path) -> Result<std::path::PathBuf> {
        std::fs::create_dir_all(dir).map_err(io(dir))?;
        let path = dir.join(format!("{}.json", self.name));
        let text = serde_json::to_string_pretty(self).map_err(json(&path))?;
        std::fs::write(&path, format!("{text}\n")).map_err(io(&path))?;
        Ok(path)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(io(path))?;
        serde_json::from_str(&text).map_err(json(path))
    }

    pub fn latest(dir: &Path) -> Result<Option<Self>> {
        let Some(path) = wre_core::store::newest_in(dir)? else {
            return Ok(None);
        };
        Ok(Some(Self::load(&path)?))
    }
}

fn counter_pattern() -> &'static regex::Regex {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"\b([A-Za-z_$][A-Za-z_$]*)(\d+)\b").expect("counter pattern compiles")
    })
}

pub fn normalise_counters(text: &str) -> String {
    counter_pattern()
        .replace_all(text, |captures: &regex::Captures| {
            format!("{}#", &captures[1])
        })
        .into_owned()
}

pub fn normalise_value(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(normalise_counters(text)),
        Value::Array(items) => Value::Array(items.iter().map(normalise_value).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, item)| (key.clone(), normalise_value(item)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MapChange {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapDiff {
    pub key: String,
    pub change: MapChange,
    #[serde(default)]
    pub before: Option<Value>,
    #[serde(default)]
    pub after: Option<Value>,
}

impl MapDiff {
    pub fn render(&self) -> String {
        match self.change {
            MapChange::Added => format!("added `{}`", self.key),
            MapChange::Removed => format!("removed `{}`", self.key),
            MapChange::Changed => format!("changed `{}`", self.key),
        }
    }
}

pub fn diff_maps(before: &Value, after: &Value, normalise: bool) -> Vec<MapDiff> {
    let left = flatten(before, normalise);
    let right = flatten(after, normalise);

    let keys: BTreeSet<&String> = left.keys().chain(right.keys()).collect();
    let mut out = Vec::new();

    for key in keys {
        match (left.get(key), right.get(key)) {
            (Some(a), Some(b)) if a != b => out.push(MapDiff {
                key: key.clone(),
                change: MapChange::Changed,
                before: Some(a.clone()),
                after: Some(b.clone()),
            }),
            (Some(a), None) => out.push(MapDiff {
                key: key.clone(),
                change: MapChange::Removed,
                before: Some(a.clone()),
                after: None,
            }),
            (None, Some(b)) => out.push(MapDiff {
                key: key.clone(),
                change: MapChange::Added,
                before: None,
                after: Some(b.clone()),
            }),
            _ => {}
        }
    }

    out
}

fn flatten(value: &Value, normalise: bool) -> BTreeMap<String, Value> {
    let source = if normalise {
        normalise_value(value)
    } else {
        value.clone()
    };

    let mut out = BTreeMap::new();

    match source {
        Value::Object(map) => {
            for (key, item) in map {
                out.insert(key, item);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.into_iter().enumerate() {
                let key = item
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| item.get("name").and_then(Value::as_str).map(str::to_string))
                    .unwrap_or_else(|| index.to_string());
                out.insert(key, item);
            }
        }
        other => {
            out.insert("value".to_string(), other);
        }
    }

    out
}

pub fn render_diff(diffs: &[MapDiff]) -> String {
    if diffs.is_empty() {
        return "No change against the baseline.\n\n".to_string();
    }

    let mut table = Table::new(&["key", "change", "before", "after"]);

    for entry in diffs {
        table.push(vec![
            format!("`{}`", entry.key),
            format!("{:?}", entry.change).to_lowercase(),
            summarise(&entry.before),
            summarise(&entry.after),
        ]);
    }

    table.render()
}

fn summarise(value: &Option<Value>) -> String {
    match value {
        None => String::new(),
        Some(Value::String(text)) if text.len() > 60 => format!("{}…", &text[..60]),
        Some(other) => {
            let text = other.to_string();
            if text.len() > 60 {
                format!("{}…", &text[..60])
            } else {
                text
            }
        }
    }
}

pub fn compare_saved(dir: &Path, name: &str, current: &Value, normalise: bool) -> Result<Vec<MapDiff>> {
    let path = dir.join(format!("{name}.json"));
    if !path.exists() {
        return Err(Error::msg(format!(
            "no baseline named {name} in {}",
            dir.display()
        )));
    }

    let baseline = Baseline::load(&path)?;
    Ok(diff_maps(&baseline.map, current, normalise))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalises_counter_names() {
        assert_eq!(normalise_counters("fn3(v12, v13)"), "fn#(v#, v#)");
        assert_eq!(normalise_counters("canvasHash"), "canvasHash");
        assert_eq!(normalise_counters("s17"), "s#");
    }

    #[test]
    fn a_pure_rename_is_silent_when_normalised() {
        let before = json!({ "s1": { "reads": "fn3(v12)" } });
        let after = json!({ "s1": { "reads": "fn7(v44)" } });

        assert!(diff_maps(&before, &after, true).is_empty());
        assert_eq!(diff_maps(&before, &after, false).len(), 1);
    }

    #[test]
    fn reports_real_changes() {
        let before = json!({ "s1": { "source": "navigator.userAgent" }, "s2": { "source": "screen.width" } });
        let after = json!({ "s1": { "source": "navigator.userAgentData" }, "s3": { "source": "audio" } });

        let diffs = diff_maps(&before, &after, true);
        let rendered: Vec<String> = diffs.iter().map(MapDiff::render).collect();

        assert!(rendered.iter().any(|line| line.contains("changed `s1`")));
        assert!(rendered.iter().any(|line| line.contains("removed `s2`")));
        assert!(rendered.iter().any(|line| line.contains("added `s3`")));
    }

    #[test]
    fn arrays_key_on_id_or_name() {
        let before = json!([{ "id": "s1", "v": 1 }, { "id": "s2", "v": 2 }]);
        let after = json!([{ "id": "s2", "v": 2 }, { "id": "s1", "v": 9 }]);

        let diffs = diff_maps(&before, &after, false);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].key, "s1");
    }

    #[test]
    fn saves_and_reloads_a_baseline() {
        let dir = std::env::temp_dir().join(format!("wre-baseline-{}", std::process::id()));
        let baseline = Baseline::new("2026-08-15", json!({ "s1": 1 }));
        let path = baseline.save(&dir).unwrap();

        let loaded = Baseline::load(&path).unwrap();
        assert_eq!(loaded.name, "2026-08-15");
        assert_eq!(loaded.map, json!({ "s1": 1 }));

        let diffs = compare_saved(&dir, "2026-08-15", &json!({ "s1": 2 }), true).unwrap();
        assert_eq!(diffs.len(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn renders_an_empty_diff_plainly() {
        assert!(render_diff(&[]).contains("No change"));
    }
}
