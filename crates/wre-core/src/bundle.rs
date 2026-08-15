use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::digest::sha256;
use crate::error::{Error, Result, io, json};
use crate::paths::safe_name;

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureBundle {
    pub schema: u32,
    pub id: String,
    pub target: String,
    pub url: String,
    pub captured_at: DateTime<Utc>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub tool: ToolInfo,
    #[serde(default)]
    pub browser: BrowserInfo,
    #[serde(default)]
    pub emulation: Vec<EmulationEntry>,
    #[serde(default)]
    pub requests: Vec<RequestRecord>,
    #[serde(default)]
    pub scripts: Vec<ScriptRecord>,
    #[serde(default)]
    pub documents: Vec<DocumentRecord>,
    #[serde(default)]
    pub cookies: Vec<CookieRecord>,
    #[serde(default)]
    pub storage: Vec<StorageRecord>,
    #[serde(default)]
    pub console: Vec<ConsoleRecord>,
    #[serde(default)]
    pub exceptions: Vec<ExceptionRecord>,
    #[serde(default)]
    pub probes: BTreeMap<String, Value>,
    #[serde(default)]
    pub notes: BTreeMap<String, Value>,
}

impl CaptureBundle {
    pub fn new(target: impl Into<String>, url: impl Into<String>) -> Self {
        let url = url.into();
        let captured_at = Utc::now();
        let target = target.into();
        let id = format!("{}-{}", safe_name(&target), captured_at.format("%Y%m%dT%H%M%SZ"));

        Self {
            schema: SCHEMA,
            id,
            target,
            url,
            captured_at,
            label: None,
            tool: ToolInfo::default(),
            browser: BrowserInfo::default(),
            emulation: Vec::new(),
            requests: Vec::new(),
            scripts: Vec::new(),
            documents: Vec::new(),
            cookies: Vec::new(),
            storage: Vec::new(),
            console: Vec::new(),
            exceptions: Vec::new(),
            probes: BTreeMap::new(),
            notes: BTreeMap::new(),
        }
    }

    pub fn read(dir: &Path) -> Result<Self> {
        let path = dir.join("bundle.json");
        let text = std::fs::read_to_string(&path).map_err(io(&path))?;
        let bundle: CaptureBundle = serde_json::from_str(&text).map_err(json(&path))?;

        if bundle.schema != SCHEMA {
            return Err(Error::BundleSchema { found: bundle.schema, expected: SCHEMA });
        }

        Ok(bundle)
    }

    pub fn write(&self, dir: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(dir).map_err(io(dir))?;
        let path = dir.join("bundle.json");
        let text = serde_json::to_string_pretty(self).map_err(json(&path))?;
        std::fs::write(&path, format!("{text}\n")).map_err(io(&path))?;
        Ok(path)
    }

    pub fn requests_matching(&self, needle: &str) -> Vec<&RequestRecord> {
        self.requests
            .iter()
            .filter(|request| request.url.contains(needle))
            .collect()
    }

    pub fn posts(&self) -> Vec<&RequestRecord> {
        self.requests
            .iter()
            .filter(|request| request.method.eq_ignore_ascii_case("POST"))
            .collect()
    }

    pub fn script_by_url(&self, needle: &str) -> Option<&ScriptRecord> {
        self.scripts.iter().find(|script| script.url.contains(needle))
    }

    pub fn largest_script(&self) -> Option<&ScriptRecord> {
        self.scripts.iter().max_by_key(|script| script.body.size)
    }

    pub fn probe<T: for<'de> Deserialize<'de>>(&self, name: &str) -> Option<T> {
        self.probes
            .get(name)
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    }

    pub fn cookie(&self, name: &str) -> Option<&CookieRecord> {
        self.cookies.iter().find(|cookie| cookie.name == name)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BrowserInfo {
    #[serde(default)]
    pub product: String,
    #[serde(default)]
    pub user_agent: String,
    #[serde(default)]
    pub protocol_version: String,
    #[serde(default)]
    pub headless: bool,
    #[serde(default)]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulationEntry {
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BodyRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default)]
    pub base64: bool,
    #[serde(default)]
    pub size: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

impl BodyRef {
    pub const INLINE_LIMIT: usize = 4096;

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn store(dir: &Path, name: &str, bytes: &[u8], text_like: bool) -> Result<Self> {
        let digest = sha256(bytes);
        let mut body = BodyRef {
            inline: None,
            file: None,
            base64: !text_like,
            size: bytes.len(),
            sha256: Some(digest),
        };

        if text_like && bytes.len() <= Self::INLINE_LIMIT {
            if let Ok(text) = std::str::from_utf8(bytes) {
                body.inline = Some(text.to_string());
                return Ok(body);
            }
        }

        let bodies = dir.join("bodies");
        std::fs::create_dir_all(&bodies).map_err(io(&bodies))?;
        let file = bodies.join(name);
        std::fs::write(&file, bytes).map_err(io(&file))?;
        body.file = Some(format!("bodies/{name}"));
        Ok(body)
    }

    pub fn load(&self, dir: &Path) -> Result<Vec<u8>> {
        if let Some(text) = &self.inline {
            return Ok(text.as_bytes().to_vec());
        }

        let Some(relative) = &self.file else {
            return Ok(Vec::new());
        };

        let path = dir.join(relative);
        std::fs::read(&path).map_err(io(&path))
    }

    pub fn text(&self, dir: &Path) -> Result<String> {
        let bytes = self.load(dir)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0 && self.inline.is_none() && self.file.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestRecord {
    pub id: String,
    pub url: String,
    pub method: String,
    #[serde(default)]
    pub resource_type: Option<String>,
    #[serde(default)]
    pub request_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub request_body: BodyRef,
    #[serde(default)]
    pub status: Option<u16>,
    #[serde(default)]
    pub response_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub response_body: BodyRef,
    #[serde(default)]
    pub remote_address: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub initiator: Option<String>,
    #[serde(default)]
    pub initiator_stack: Vec<String>,
    #[serde(default)]
    pub sent_cookies: Vec<String>,
    #[serde(default)]
    pub set_cookies: Vec<String>,
    #[serde(default)]
    pub wall_time: Option<f64>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptRecord {
    pub url: String,
    #[serde(default)]
    pub status: Option<u16>,
    #[serde(default)]
    pub body: BodyRef,
    #[serde(default)]
    pub served: Option<BodyRef>,
    #[serde(default)]
    pub rewritten: bool,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentRecord {
    pub url: String,
    #[serde(default)]
    pub frame: Option<String>,
    #[serde(default)]
    pub body: BodyRef,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieRecord {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub expires: Option<f64>,
    #[serde(default)]
    pub http_only: bool,
    #[serde(default)]
    pub secure: bool,
    #[serde(default)]
    pub same_site: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageRecord {
    pub origin: String,
    pub kind: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleRecord {
    pub level: String,
    pub text: String,
    #[serde(default)]
    pub at: Option<f64>,
    #[serde(default)]
    pub stack: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExceptionRecord {
    pub text: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub column: Option<u32>,
    #[serde(default)]
    pub description: Option<String>,
}
