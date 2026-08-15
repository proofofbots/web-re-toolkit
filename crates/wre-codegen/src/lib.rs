pub mod binaries;
pub mod go;
pub mod names;
pub mod node;
pub mod python;
pub mod rust;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use wre_client::spec::{BundleDescriptor, ClientDescriptor};
use wre_core::error::{Error, Result, io};

pub use binaries::{BinaryEntry, Binaries};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PackageConfig {
    pub version: String,
    pub node_scope: String,
    pub python_prefix: String,
    pub go_module: String,
    pub rust_prefix: String,
    pub license: String,
    pub repository: String,
    pub download_url: String,
    pub node_runtime: String,
    pub python_runtime: String,
    pub go_runtime: String,
    pub go_runtime_replace: Option<String>,
    pub rust_runtime_path: Option<String>,
}

impl Default for PackageConfig {
    fn default() -> Self {
        Self {
            version: "0.1.0".to_string(),
            node_scope: "@wre".to_string(),
            python_prefix: "wre-".to_string(),
            go_module: "github.com/proofofbots/web-re-toolkit/packages/go/clients".to_string(),
            rust_prefix: "wre-sdk-".to_string(),
            license: "MIT".to_string(),
            repository: "https://github.com/proofofbots/web-re-toolkit".to_string(),
            download_url:
                "https://github.com/proofofbots/web-re-toolkit/releases/download/v{version}/wred-{triple}"
                    .to_string(),
            node_runtime: "^0.1.0".to_string(),
            python_runtime: ">=0.1.0".to_string(),
            go_runtime: "github.com/proofofbots/web-re-toolkit/packages/go/wre v0.1.0".to_string(),
            go_runtime_replace: None,
            rust_runtime_path: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Node,
    Python,
    Go,
    Rust,
}

impl Language {
    pub const ALL: [Language; 4] = [Language::Node, Language::Python, Language::Go, Language::Rust];

    pub fn name(&self) -> &'static str {
        match self {
            Language::Node => "node",
            Language::Python => "python",
            Language::Go => "go",
            Language::Rust => "rust",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_lowercase().as_str() {
            "node" | "nodejs" | "js" | "javascript" => Ok(Language::Node),
            "python" | "py" => Ok(Language::Python),
            "go" | "golang" => Ok(Language::Go),
            "rust" | "rs" => Ok(Language::Rust),
            other => Err(Error::msg(format!(
                "unknown language {other}, pick node, python, go or rust"
            ))),
        }
    }

    pub fn parse_list(values: &[String]) -> Result<Vec<Self>> {
        if values.is_empty() {
            return Ok(Language::ALL.to_vec());
        }

        let mut out = Vec::new();
        for value in values {
            for part in value.split(',') {
                if part.trim().is_empty() {
                    continue;
                }
                if part.trim().eq_ignore_ascii_case("all") {
                    return Ok(Language::ALL.to_vec());
                }
                let language = Language::parse(part)?;
                if !out.contains(&language) {
                    out.push(language);
                }
            }
        }
        Ok(out)
    }
}

pub struct Plan<'a> {
    pub bundle: &'a BundleDescriptor,
    pub client: &'a ClientDescriptor,
    pub config: &'a PackageConfig,
    pub binaries: &'a Binaries,
    pub out: PathBuf,
}

impl Plan<'_> {
    pub fn schema_hash(&self) -> String {
        self.bundle.schema_hash()
    }

    pub fn root(&self, language: Language) -> PathBuf {
        self.out.join(language.name()).join(&self.client.id)
    }
}

#[derive(Debug, Clone)]
pub struct Emitted {
    pub language: Language,
    pub root: PathBuf,
    pub files: Vec<PathBuf>,
}

pub fn emit(language: Language, plan: &Plan) -> Result<Emitted> {
    let files = match language {
        Language::Node => node::emit(plan),
        Language::Python => python::emit(plan),
        Language::Go => go::emit(plan),
        Language::Rust => rust::emit(plan),
    }?;

    Ok(Emitted { language, root: plan.root(language), files })
}

pub fn emit_all(languages: &[Language], plan: &Plan) -> Result<Vec<Emitted>> {
    let mut out = Vec::new();
    for language in languages {
        out.push(emit(*language, plan)?);
    }
    Ok(out)
}

pub(crate) fn write(path: &Path, text: &str) -> Result<PathBuf> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io(parent))?;
    }
    std::fs::write(path, text).map_err(io(path))?;
    Ok(path.to_path_buf())
}

pub(crate) fn copy_binary(from: &Path, to: &Path) -> Result<PathBuf> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(io(parent))?;
    }

    std::fs::copy(from, to).map_err(io(from))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(to).map_err(io(to))?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(to, permissions).map_err(io(to))?;
    }

    Ok(to.to_path_buf())
}

pub(crate) fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\""))
}

pub(crate) fn summary_line(text: &str, fallback: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() { fallback.to_string() } else { trimmed.replace('\n', " ") }
}

pub fn download_url(config: &PackageConfig, triple: &str) -> String {
    config
        .download_url
        .replace("{version}", &config.version)
        .replace("{triple}", triple)
}
