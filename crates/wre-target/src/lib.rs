use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use wre_core::error::{Error, Result, io};
use wre_js::pipeline::{Config, MemberReadSpec, RenameConfig, SourceKind};
use wre_js::surface::SignatureRule;
use wre_live::mount::{MountPlan, SourcePatch};
use wre_probe::{MethodTrap, PropertyTrap, SurfaceSpec};
use wre_vm::probe::FrameModel;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub pages: BTreeMap<String, String>,
    #[serde(default)]
    pub discovery: Discovery,
    #[serde(default)]
    pub deobfuscate: Deobfuscate,
    #[serde(default)]
    pub live: Live,
    #[serde(default)]
    pub vm: Option<Vm>,
    #[serde(default)]
    pub wire: Wire,
    #[serde(default)]
    pub knobs: Vec<KnobSpec>,
    #[serde(default)]
    pub probe: Probe,
    #[serde(default)]
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Discovery {
    #[serde(default)]
    pub script_patterns: Vec<String>,
    #[serde(default)]
    pub endpoint_patterns: Vec<String>,
    #[serde(default)]
    pub cookie_names: Vec<String>,
    #[serde(default)]
    pub header_names: Vec<String>,
    #[serde(default)]
    pub document_markers: Vec<String>,
}

impl Discovery {
    pub fn find_scripts(&self, document: &str) -> Result<Vec<String>> {
        let mut out = Vec::new();

        for pattern in &self.script_patterns {
            let regex = regex::Regex::new(pattern)
                .map_err(|error| Error::msg(format!("script pattern {pattern}: {error}")))?;

            for capture in regex.captures_iter(document) {
                let value = capture
                    .get(1)
                    .or_else(|| capture.get(0))
                    .map(|found| found.as_str().to_string());

                if let Some(value) = value {
                    if !out.contains(&value) {
                        out.push(value);
                    }
                }
            }
        }

        Ok(out)
    }

    pub fn marks(&self, document: &str) -> Vec<String> {
        self.document_markers
            .iter()
            .filter(|marker| document.contains(marker.as_str()))
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Deobfuscate {
    #[serde(default)]
    pub source_kind: SourceKind,
    #[serde(default = "default_sweeps")]
    pub max_sweeps: usize,
    #[serde(default)]
    pub rename: bool,
    #[serde(default)]
    pub infer_names: bool,
    #[serde(default)]
    pub remove_unused: bool,
    #[serde(default)]
    pub inline_global_aliases: bool,
    #[serde(default)]
    pub aggressive_member_access: bool,
    #[serde(default)]
    pub drop_debugger: bool,
    #[serde(default)]
    pub member_reads: Vec<MemberReadSpec>,
    #[serde(default)]
    pub hash_functions: Vec<String>,
    #[serde(default)]
    pub reserved_names: Vec<String>,
    #[serde(default)]
    pub forced_names: BTreeMap<String, String>,
    #[serde(default)]
    pub disabled_passes: Vec<String>,
}

fn default_sweeps() -> usize {
    8
}

impl Deobfuscate {
    pub fn to_config(&self) -> Config {
        Config {
            max_sweeps: self.max_sweeps.max(1),
            member_reads: self.member_reads.clone(),
            hash_functions: self.hash_functions.clone(),
            rename: RenameConfig {
                enabled: self.rename,
                infer: self.infer_names,
                reserved: self.reserved_names.iter().cloned().collect(),
                forced: self.forced_names.clone(),
            },
            inline_global_aliases: self.inline_global_aliases,
            aggressive_member_access: self.aggressive_member_access,
            drop_debugger: self.drop_debugger,
            remove_unused: self.remove_unused,
            source_type: self.source_kind,
            ..Config::default()
        }
    }

    pub fn pipeline(&self) -> wre_js::pipeline::Pipeline {
        let disabled: Vec<&str> = self.disabled_passes.iter().map(String::as_str).collect();
        wre_js::standard_pipeline().without(&disabled)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Live {
    #[serde(default)]
    pub signatures: Vec<Signature>,
    #[serde(default)]
    pub patches: Vec<Patch>,
    #[serde(default)]
    pub exports: BTreeMap<String, String>,
    #[serde(default)]
    pub prelude: Vec<String>,
    #[serde(default)]
    pub after: Vec<String>,
    #[serde(default)]
    pub tolerate_throw: bool,
    #[serde(default)]
    pub clock_ms: Option<f64>,
    #[serde(default)]
    pub random_seed: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Signature {
    pub role: String,
    pub pattern: String,
    #[serde(default)]
    pub params: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Patch {
    pub find: String,
    pub replace: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub limit: usize,
    #[serde(default)]
    pub required: bool,
}

impl Live {
    pub fn to_plan(&self, source_kind: SourceKind) -> MountPlan {
        MountPlan {
            prelude: self.prelude.clone(),
            patches: self
                .patches
                .iter()
                .map(|patch| SourcePatch {
                    find: patch.find.clone(),
                    replace: patch.replace.clone(),
                    regex: patch.regex,
                    limit: patch.limit,
                    required: patch.required,
                })
                .collect(),
            exports: self.exports.clone(),
            after: self.after.clone(),
            signatures: self
                .signatures
                .iter()
                .map(|entry| SignatureRule {
                    role: entry.role.clone(),
                    pattern: entry.pattern.clone(),
                    params: entry.params,
                })
                .collect(),
            source_kind,
            tolerate_throw: self.tolerate_throw,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Vm {
    #[serde(default)]
    pub handler_table: Option<String>,
    #[serde(default)]
    pub dispatch_hint: Option<String>,
    #[serde(default)]
    pub frame_model: Option<String>,
    #[serde(default)]
    pub opcode_labels: BTreeMap<String, String>,
    #[serde(default)]
    pub entry: usize,
}

impl Vm {
    pub fn to_frame_model(&self) -> Option<FrameModel> {
        self.frame_model
            .as_ref()
            .map(|source| FrameModel::new(source.clone()))
    }

    pub fn labels(&self) -> BTreeMap<u32, String> {
        self.opcode_labels
            .iter()
            .filter_map(|(key, value)| key.parse::<u32>().ok().map(|opcode| (opcode, value.clone())))
            .collect()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Wire {
    #[serde(default)]
    pub codec: CodecChoice,
    #[serde(default)]
    pub xor_key_hex: Option<String>,
    #[serde(default)]
    pub open_role: Option<String>,
    #[serde(default)]
    pub seal_role: Option<String>,
    #[serde(default)]
    pub request_patterns: Vec<String>,
    #[serde(default)]
    pub field_labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodecChoice {
    #[default]
    Json,
    Base64Json,
    DeflateRawJson,
    DeflateJson,
    Xor,
    Live,
}

impl Wire {
    pub fn matches_request(&self, url: &str) -> Result<bool> {
        for pattern in &self.request_patterns {
            let regex = regex::Regex::new(pattern)
                .map_err(|error| Error::msg(format!("request pattern {pattern}: {error}")))?;
            if regex.is_match(url) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn label(&self, address: &str) -> Option<&str> {
        self.field_labels.get(address).map(String::as_str)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KnobSpec {
    pub name: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub emulation: Vec<EmulationSpec>,
    #[serde(default)]
    pub inject: Option<String>,
    #[serde(default)]
    pub patches: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmulationSpec {
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Probe {
    #[serde(default)]
    pub preset: ProbePreset,
    #[serde(default)]
    pub properties: Vec<PropertySpec>,
    #[serde(default)]
    pub methods: Vec<MethodSpec>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub workers: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbePreset {
    #[default]
    Fingerprint,
    Minimal,
    None,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PropertySpec {
    pub holder: String,
    pub property: String,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MethodSpec {
    pub holder: String,
    pub method: String,
    #[serde(default)]
    pub label: Option<String>,
}

impl Probe {
    pub fn to_spec(&self) -> SurfaceSpec {
        let mut spec = match self.preset {
            ProbePreset::Fingerprint => wre_probe::fingerprint_surface(),
            ProbePreset::Minimal => wre_probe::minimal_surface(),
            ProbePreset::None => SurfaceSpec::default(),
        };

        for entry in &self.properties {
            spec.properties.push(PropertyTrap {
                holder: entry.holder.clone(),
                property: entry.property.clone(),
                label: entry.label.clone(),
            });
        }

        for entry in &self.methods {
            spec.methods.push(MethodTrap {
                holder: entry.holder.clone(),
                method: entry.method.clone(),
                label: entry.label.clone(),
            });
        }

        spec.events.extend(self.events.clone());
        spec.network |= self.network;
        spec.workers |= self.workers;
        spec
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    #[serde(default)]
    pub kind: CheckKind,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub expression: Option<String>,
    #[serde(default)]
    pub expect: Option<Value>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckKind {
    #[default]
    Roundtrip,
    Deobfuscate,
    Mount,
    Expression,
    VmDecode,
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(io(path))?;

        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("toml");

        let manifest: Manifest = match extension {
            "json" => serde_json::from_str(&text)
                .map_err(|error| Error::msg(format!("{}: {error}", path.display())))?,
            _ => toml::from_str(&text)
                .map_err(|error| Error::msg(format!("{}: {error}", path.display())))?,
        };

        manifest.validate()?;
        Ok(manifest)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self)
            .map_err(|error| Error::msg(format!("manifest did not serialise: {error}")))?;
        std::fs::write(path, text).map_err(io(path))
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::msg("manifest has no name"));
        }

        for pattern in self
            .discovery
            .script_patterns
            .iter()
            .chain(self.discovery.endpoint_patterns.iter())
            .chain(self.wire.request_patterns.iter())
        {
            regex::Regex::new(pattern)
                .map_err(|error| Error::msg(format!("bad pattern {pattern}: {error}")))?;
        }

        for signature in &self.live.signatures {
            regex::Regex::new(&signature.pattern).map_err(|error| {
                Error::msg(format!(
                    "signature for {} does not compile: {error}",
                    signature.role
                ))
            })?;
        }

        for patch in &self.live.patches {
            if patch.regex {
                regex::Regex::new(&patch.find).map_err(|error| {
                    Error::msg(format!("patch pattern {} does not compile: {error}", patch.find))
                })?;
            }
        }

        let mut names: Vec<&str> = self.knobs.iter().map(|knob| knob.name.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        if before != names.len() {
            return Err(Error::msg("two knobs share a name"));
        }

        Ok(())
    }

    pub fn page(&self, name: &str) -> Option<&str> {
        self.pages.get(name).map(String::as_str)
    }

    pub fn first_url(&self) -> Option<&str> {
        self.urls.first().map(String::as_str)
    }

    pub fn example() -> Self {
        Manifest {
            name: "example".to_string(),
            description: "a worked adapter showing every section".to_string(),
            urls: vec!["https://example.test/".to_string()],
            pages: [("login".to_string(), "https://example.test/login".to_string())]
                .into_iter()
                .collect(),
            discovery: Discovery {
                script_patterns: vec![r#"src="([^"]*/collect\.js[^"]*)""#.to_string()],
                endpoint_patterns: vec![r"/v\d+/collect".to_string()],
                cookie_names: vec!["_session".to_string()],
                header_names: vec!["x-signature".to_string()],
                document_markers: vec!["window.__collector".to_string()],
            },
            deobfuscate: Deobfuscate {
                max_sweeps: 8,
                rename: true,
                infer_names: true,
                remove_unused: true,
                inline_global_aliases: true,
                ..Deobfuscate::default()
            },
            live: Live {
                signatures: vec![Signature {
                    role: "hash".to_string(),
                    pattern: "2166136261|0x811c9dc5".to_string(),
                    params: Some(1),
                }],
                tolerate_throw: true,
                clock_ms: Some(1_700_000_000_000.0),
                random_seed: Some(1),
                ..Live::default()
            },
            vm: None,
            wire: Wire {
                codec: CodecChoice::Base64Json,
                request_patterns: vec![r"/v\d+/collect".to_string()],
                ..Wire::default()
            },
            knobs: vec![KnobSpec {
                name: "timezone-berlin".to_string(),
                group: "locale".to_string(),
                emulation: vec![EmulationSpec {
                    method: "Emulation.setTimezoneOverride".to_string(),
                    params: serde_json::json!({ "timezoneId": "Europe/Berlin" }),
                }],
                ..KnobSpec::default()
            }],
            probe: Probe {
                preset: ProbePreset::Fingerprint,
                network: true,
                ..Probe::default()
            },
            checks: vec![Check {
                name: "payload round trips".to_string(),
                kind: CheckKind::Roundtrip,
                note: "every captured body decodes and re-encodes byte for byte".to_string(),
                ..Check::default()
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_manifest_round_trips_through_toml() {
        let manifest = Manifest::example();
        let text = toml::to_string_pretty(&manifest).unwrap();
        let back: Manifest = toml::from_str(&text).unwrap();

        assert_eq!(back.name, "example");
        assert_eq!(back.knobs.len(), 1);
        assert_eq!(back.wire.codec, CodecChoice::Base64Json);
        back.validate().unwrap();
    }

    #[test]
    fn validation_catches_bad_patterns() {
        let mut manifest = Manifest::example();
        manifest.discovery.script_patterns = vec!["([unclosed".to_string()];
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn validation_catches_duplicate_knobs() {
        let mut manifest = Manifest::example();
        let knob = manifest.knobs[0].clone();
        manifest.knobs.push(knob);
        assert!(manifest.validate().unwrap_err().to_string().contains("share a name"));
    }

    #[test]
    fn discovery_finds_scripts_in_a_document() {
        let manifest = Manifest::example();
        let document = r#"<html><script src="https://cdn.example.test/a/collect.js?v=3"></script></html>"#;
        let found = manifest.discovery.find_scripts(document).unwrap();

        assert_eq!(found, vec!["https://cdn.example.test/a/collect.js?v=3".to_string()]);
        assert!(manifest.discovery.marks("var x = window.__collector;").len() == 1);
    }

    #[test]
    fn deobfuscate_section_builds_a_pipeline_config() {
        let manifest = Manifest::example();
        let config = manifest.deobfuscate.to_config();

        assert!(config.rename.enabled);
        assert!(config.remove_unused);
        assert_eq!(config.max_sweeps, 8);
        assert_eq!(manifest.deobfuscate.pipeline().passes().len(), wre_js::REGISTRY.len());
    }

    #[test]
    fn disabled_passes_are_dropped_from_the_pipeline() {
        let mut manifest = Manifest::example();
        manifest.deobfuscate.disabled_passes = vec!["rename-identifiers".to_string()];

        let pipeline = manifest.deobfuscate.pipeline();
        assert!(!pipeline.passes().iter().any(|pass| pass.name == "rename-identifiers"));
    }

    #[test]
    fn live_section_becomes_a_mount_plan() {
        let manifest = Manifest::example();
        let plan = manifest.live.to_plan(SourceKind::Script);

        assert_eq!(plan.signatures.len(), 1);
        assert_eq!(plan.signatures[0].role, "hash");
        assert!(plan.tolerate_throw);
    }

    #[test]
    fn probe_section_builds_a_surface() {
        let manifest = Manifest::example();
        let spec = manifest.probe.to_spec();
        assert!(spec.network);
        assert!(spec.properties.len() > 10);
    }

    #[test]
    fn wire_section_matches_request_urls() {
        let manifest = Manifest::example();
        assert!(manifest.wire.matches_request("https://x.test/v2/collect").unwrap());
        assert!(!manifest.wire.matches_request("https://x.test/other").unwrap());
    }
}
