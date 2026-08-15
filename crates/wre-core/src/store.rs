use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{Error, Result, io, json};
use crate::paths::Workspace;

#[derive(Debug, Clone)]
pub struct Store {
    pub root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn for_workspace(workspace: &Workspace, kind: &str) -> Self {
        Self::new(workspace.artifact(kind))
    }

    pub fn ensure(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root).map_err(io(&self.root))
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    pub fn write_bytes(&self, name: &str, bytes: &[u8]) -> Result<PathBuf> {
        self.ensure()?;
        let path = self.path(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        std::fs::write(&path, bytes).map_err(io(&path))?;
        Ok(path)
    }

    pub fn write_text(&self, name: &str, text: &str) -> Result<PathBuf> {
        self.write_bytes(name, text.as_bytes())
    }

    pub fn write_json<T: Serialize>(&self, name: &str, value: &T) -> Result<PathBuf> {
        let path = self.path(name);
        let text = serde_json::to_string_pretty(value).map_err(json(&path))?;
        self.write_text(name, &format!("{text}\n"))
    }

    pub fn read_bytes(&self, name: &str) -> Result<Vec<u8>> {
        let path = self.path(name);
        std::fs::read(&path).map_err(io(&path))
    }

    pub fn read_text(&self, name: &str) -> Result<String> {
        let path = self.path(name);
        std::fs::read_to_string(&path).map_err(io(&path))
    }

    pub fn read_json<T: DeserializeOwned>(&self, name: &str) -> Result<T> {
        let path = self.path(name);
        let text = self.read_text(name)?;
        serde_json::from_str(&text).map_err(json(&path))
    }

    pub fn exists(&self, name: &str) -> bool {
        self.path(name).exists()
    }

    pub fn entries(&self) -> Result<Vec<PathBuf>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for entry in std::fs::read_dir(&self.root).map_err(io(&self.root))? {
            let entry = entry.map_err(io(&self.root))?;
            out.push(entry.path());
        }
        out.sort();
        Ok(out)
    }

    pub fn newest(&self) -> Result<Option<PathBuf>> {
        Ok(newest_in(&self.root)?)
    }

    pub fn newest_matching(&self, predicate: impl Fn(&Path) -> bool) -> Result<Option<PathBuf>> {
        let mut best: Option<(SystemTime, PathBuf)> = None;

        for path in self.entries()? {
            if !predicate(&path) {
                continue;
            }
            let modified = modified_at(&path)?;
            if best.as_ref().is_none_or(|(time, _)| modified > *time) {
                best = Some((modified, path));
            }
        }

        Ok(best.map(|(_, path)| path))
    }

    pub fn require_newest(&self) -> Result<PathBuf> {
        self.newest()?
            .ok_or_else(|| Error::ArtifactMissing(self.root.display().to_string()))
    }
}

pub fn modified_at(path: &Path) -> Result<SystemTime> {
    let meta = std::fs::metadata(path).map_err(io(path))?;
    meta.modified().map_err(io(path))
}

pub fn newest_in(root: &Path) -> Result<Option<PathBuf>> {
    if !root.exists() {
        return Ok(None);
    }

    let mut best: Option<(SystemTime, PathBuf)> = None;

    for entry in std::fs::read_dir(root).map_err(io(root))? {
        let entry = entry.map_err(io(root))?;
        let path = entry.path();
        let modified = modified_at(&path)?;
        if best.as_ref().is_none_or(|(time, _)| modified > *time) {
            best = Some((modified, path));
        }
    }

    Ok(best.map(|(_, path)| path))
}

pub fn copy_tree(from: &Path, to: &Path) -> Result<usize> {
    std::fs::create_dir_all(to).map_err(io(to))?;
    let mut copied = 0usize;

    for entry in walkdir::WalkDir::new(from).into_iter().filter_map(|e| e.ok()) {
        let relative = match entry.path().strip_prefix(from) {
            Ok(value) => value,
            Err(_) => continue,
        };

        if relative.as_os_str().is_empty() {
            continue;
        }

        let destination = to.join(relative);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&destination).map_err(io(&destination))?;
        } else {
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent).map_err(io(parent))?;
            }
            std::fs::copy(entry.path(), &destination).map_err(io(entry.path()))?;
            copied += 1;
        }
    }

    Ok(copied)
}
