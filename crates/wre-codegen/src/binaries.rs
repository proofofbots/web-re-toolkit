use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result, io};

use crate::names::binary_name;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryEntry {
    pub triple: String,
    pub file: String,
    pub sha256: String,
    pub size: u64,
    #[serde(skip)]
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Binaries {
    pub entries: Vec<BinaryEntry>,
}

impl Binaries {
    pub fn collect(root: &Path) -> Result<Self> {
        let mut entries = Vec::new();

        if !root.is_dir() {
            return Ok(Self { entries });
        }

        let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
            .map_err(io(root))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        dirs.sort();

        for dir in dirs {
            let triple = dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();

            let file = binary_name(&triple);
            let path = dir.join(file);
            if !path.is_file() {
                continue;
            }

            let bytes = std::fs::read(&path).map_err(io(&path))?;
            entries.push(BinaryEntry {
                triple,
                file: file.to_string(),
                sha256: wre_core::digest::sha256(&bytes),
                size: bytes.len() as u64,
                path,
            });
        }

        Ok(Self { entries })
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn triples(&self) -> Vec<String> {
        self.entries.iter().map(|entry| entry.triple.clone()).collect()
    }

    pub fn find(&self, triple: &str) -> Option<&BinaryEntry> {
        self.entries.iter().find(|entry| entry.triple == triple)
    }

    pub fn require(&self, triple: &str) -> Result<&BinaryEntry> {
        self.find(triple).ok_or_else(|| {
            Error::msg(format!(
                "no binary for {triple}, this bundle has {}",
                if self.entries.is_empty() {
                    "none".to_string()
                } else {
                    self.triples().join(", ")
                }
            ))
        })
    }
}
