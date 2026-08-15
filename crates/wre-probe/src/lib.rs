pub mod runtime;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use wre_core::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyTrap {
    pub holder: String,
    pub property: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl PropertyTrap {
    pub fn new(holder: &str, property: &str) -> Self {
        Self {
            holder: holder.to_string(),
            property: property.to_string(),
            label: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodTrap {
    pub holder: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl MethodTrap {
    pub fn new(holder: &str, method: &str) -> Self {
        Self {
            holder: holder.to_string(),
            method: method.to_string(),
            label: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfaceSpec {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub properties: Vec<PropertyTrap>,
    #[serde(default)]
    pub methods: Vec<MethodTrap>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub workers: bool,
    #[serde(default)]
    pub stealth: bool,
    #[serde(default)]
    pub call_sites: bool,
    #[serde(default = "default_samples")]
    pub max_samples: usize,
    #[serde(default = "default_sites")]
    pub max_sites: usize,
    #[serde(default = "default_arguments")]
    pub max_arguments: usize,
    #[serde(default = "default_sample_length")]
    pub max_sample_length: usize,
    #[serde(default = "default_network")]
    pub max_network: usize,
    #[serde(default = "default_events")]
    pub max_events: usize,
    #[serde(default = "default_blob_length")]
    pub max_blob_length: usize,
}

fn default_samples() -> usize {
    4
}

fn default_sites() -> usize {
    2
}

fn default_arguments() -> usize {
    6
}

fn default_sample_length() -> usize {
    200
}

fn default_network() -> usize {
    400
}

fn default_events() -> usize {
    400
}

fn default_blob_length() -> usize {
    200_000
}

impl Default for SurfaceSpec {
    fn default() -> Self {
        Self {
            name: "__WRE".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            properties: Vec::new(),
            methods: Vec::new(),
            events: Vec::new(),
            network: true,
            workers: false,
            stealth: true,
            call_sites: true,
            max_samples: default_samples(),
            max_sites: default_sites(),
            max_arguments: default_arguments(),
            max_sample_length: default_sample_length(),
            max_network: default_network(),
            max_events: default_events(),
            max_blob_length: default_blob_length(),
        }
    }
}

impl SurfaceSpec {
    pub fn property(mut self, holder: &str, property: &str) -> Self {
        self.properties.push(PropertyTrap::new(holder, property));
        self
    }

    pub fn method(mut self, holder: &str, method: &str) -> Self {
        self.methods.push(MethodTrap::new(holder, method));
        self
    }

    pub fn event(mut self, name: &str) -> Self {
        self.events.push(name.to_string());
        self
    }

    pub fn merge(mut self, other: SurfaceSpec) -> Self {
        self.properties.extend(other.properties);
        self.methods.extend(other.methods);
        self.events.extend(other.events);
        self.network |= other.network;
        self.workers |= other.workers;
        self
    }

    pub fn config(&self) -> Value {
        serde_json::json!({
            "name": self.name,
            "version": self.version,
            "properties": self.properties,
            "methods": self.methods,
            "events": self.events,
            "network": self.network,
            "workers": self.workers,
            "stealth": self.stealth,
            "callSites": self.call_sites,
            "maxSamples": self.max_samples,
            "maxSites": self.max_sites,
            "maxArguments": self.max_arguments,
            "maxSampleLength": self.max_sample_length,
            "maxNetwork": self.max_network,
            "maxEvents": self.max_events,
            "maxBlobLength": self.max_blob_length,
        })
    }

    pub fn build(&self) -> Result<String> {
        let config = serde_json::to_string(&self.config())
            .map_err(|error| Error::msg(format!("probe config did not serialise: {error}")))?;
        Ok(runtime::RUNTIME.replace("__WRE_CONFIG__", &config))
    }

    pub fn dump_expression(&self) -> String {
        format!(
            "(typeof {name} !== 'undefined' && {name} ? {name}.dump() : null)",
            name = self.name
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProbeDump {
    #[serde(default)]
    pub started_at: f64,
    #[serde(default)]
    pub elapsed: f64,
    #[serde(default)]
    pub reads: Vec<Entry>,
    #[serde(default)]
    pub calls: Vec<Entry>,
    #[serde(default)]
    pub events: Vec<Value>,
    #[serde(default)]
    pub network: Vec<Value>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Entry {
    pub key: String,
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub first: f64,
    #[serde(default)]
    pub last: f64,
    #[serde(default)]
    pub samples: Vec<String>,
    #[serde(default)]
    pub sites: Vec<String>,
    #[serde(default)]
    pub results: Vec<String>,
    #[serde(default)]
    pub threw: Option<String>,
}

impl ProbeDump {
    pub fn parse(value: &Value) -> Result<Self> {
        let mut dump = ProbeDump::default();

        dump.started_at = value.get("startedAt").and_then(Value::as_f64).unwrap_or(0.0);
        dump.elapsed = value.get("elapsed").and_then(Value::as_f64).unwrap_or(0.0);

        for (field, sink) in [("reads", &mut dump.reads), ("calls", &mut dump.calls)] {
            if let Some(list) = value.get(field).and_then(Value::as_array) {
                for item in list {
                    if let Ok(entry) = serde_json::from_value::<Entry>(item.clone()) {
                        sink.push(entry);
                    }
                }
            }
        }

        dump.events = value
            .get("events")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        dump.network = value
            .get("network")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        dump.notes = value
            .get("notes")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        Ok(dump)
    }

    pub fn touched(&self) -> Vec<&str> {
        self.reads
            .iter()
            .chain(self.calls.iter())
            .map(|entry| entry.key.as_str())
            .collect()
    }

    pub fn posts(&self) -> Vec<&Value> {
        self.network
            .iter()
            .filter(|entry| {
                entry
                    .get("method")
                    .and_then(Value::as_str)
                    .map(|method| method.eq_ignore_ascii_case("POST"))
                    .unwrap_or(false)
            })
            .collect()
    }
}

pub fn fingerprint_surface() -> SurfaceSpec {
    let mut spec = SurfaceSpec::default();

    for property in [
        "userAgent",
        "appVersion",
        "platform",
        "vendor",
        "language",
        "languages",
        "hardwareConcurrency",
        "deviceMemory",
        "maxTouchPoints",
        "webdriver",
        "plugins",
        "mimeTypes",
        "cookieEnabled",
        "doNotTrack",
        "userAgentData",
        "pdfViewerEnabled",
    ] {
        spec.properties.push(PropertyTrap::new("Navigator.prototype", property));
    }

    for property in [
        "width",
        "height",
        "availWidth",
        "availHeight",
        "colorDepth",
        "pixelDepth",
    ] {
        spec.properties.push(PropertyTrap::new("Screen.prototype", property));
    }

    for property in ["devicePixelRatio", "innerWidth", "innerHeight", "outerWidth", "outerHeight"] {
        spec.properties.push(PropertyTrap::new("window", property));
    }

    for method in ["toDataURL", "toBlob", "getContext"] {
        spec.methods.push(MethodTrap::new("HTMLCanvasElement.prototype", method));
    }

    for method in ["fillText", "strokeText", "getImageData", "measureText", "isPointInPath"] {
        spec.methods
            .push(MethodTrap::new("CanvasRenderingContext2D.prototype", method));
    }

    for method in ["getParameter", "getSupportedExtensions", "getExtension", "readPixels"] {
        spec.methods
            .push(MethodTrap::new("WebGLRenderingContext.prototype", method));
    }

    spec.methods.push(MethodTrap::new("AudioContext.prototype", "createOscillator"));
    spec.methods.push(MethodTrap::new("AudioContext.prototype", "createAnalyser"));
    spec.methods.push(MethodTrap::new("window", "matchMedia"));
    spec.methods.push(MethodTrap::new("window", "getComputedStyle"));
    spec.methods.push(MethodTrap::new("Intl", "DateTimeFormat"));
    spec.methods.push(MethodTrap::new("Storage.prototype", "getItem"));
    spec.methods.push(MethodTrap::new("Storage.prototype", "setItem"));
    spec.methods.push(MethodTrap::new("Document.prototype", "createElement"));
    spec.methods.push(MethodTrap::new("Element.prototype", "getBoundingClientRect"));
    spec.methods.push(MethodTrap::new("Performance.prototype", "now"));

    spec.events = ["mousemove", "mousedown", "keydown", "touchstart", "scroll", "devicemotion"]
        .into_iter()
        .map(str::to_string)
        .collect();

    spec.workers = true;
    spec
}

pub fn minimal_surface() -> SurfaceSpec {
    SurfaceSpec {
        properties: vec![PropertyTrap::new("Navigator.prototype", "userAgent")],
        methods: Vec::new(),
        events: Vec::new(),
        network: true,
        workers: false,
        call_sites: false,
        ..SurfaceSpec::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_script_with_the_config_inlined() {
        let spec = SurfaceSpec::default()
            .property("Navigator.prototype", "userAgent")
            .method("HTMLCanvasElement.prototype", "toDataURL");

        let script = spec.build().unwrap();

        assert!(!script.contains("__WRE_CONFIG__"));
        assert!(script.contains("\"userAgent\""));
        assert!(script.contains("\"toDataURL\""));
        assert!(script.contains("Object.defineProperty(root, config.name"));
    }

    #[test]
    fn the_fingerprint_preset_covers_the_usual_surfaces() {
        let spec = fingerprint_surface();
        let keys: Vec<String> = spec
            .properties
            .iter()
            .map(|trap| format!("{}.{}", trap.holder, trap.property))
            .collect();

        assert!(keys.contains(&"Navigator.prototype.webdriver".to_string()));
        assert!(keys.contains(&"Screen.prototype.colorDepth".to_string()));
        assert!(spec.methods.iter().any(|trap| trap.method == "getParameter"));
        assert!(spec.workers);
        assert!(spec.build().unwrap().len() > 1000);
    }

    #[test]
    fn merging_two_specs_keeps_both_surfaces() {
        let left = SurfaceSpec::default().property("window", "innerWidth");
        let right = SurfaceSpec::default().method("window", "matchMedia");
        let merged = left.merge(right);

        assert_eq!(merged.properties.len(), 1);
        assert_eq!(merged.methods.len(), 1);
    }

    #[test]
    fn parses_a_dump() {
        let value = serde_json::json!({
            "startedAt": 100.0,
            "elapsed": 42.0,
            "reads": [{ "key": "Navigator.prototype.userAgent", "count": 3, "samples": ["Mozilla"] }],
            "calls": [{ "key": "HTMLCanvasElement.prototype.toDataURL", "count": 1 }],
            "network": [{ "via": "fetch", "url": "https://x.test/collect", "method": "POST" }],
            "notes": ["missing holder Foo"]
        });

        let dump = ProbeDump::parse(&value).unwrap();
        assert_eq!(dump.reads[0].count, 3);
        assert_eq!(dump.posts().len(), 1);
        assert!(dump.touched().contains(&"Navigator.prototype.userAgent"));
        assert_eq!(dump.notes.len(), 1);
    }

    #[test]
    fn dump_expression_is_guarded() {
        let spec = SurfaceSpec::default();
        assert!(spec.dump_expression().contains("typeof __WRE"));
    }
}
