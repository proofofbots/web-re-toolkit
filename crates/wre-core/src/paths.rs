use std::path::{Path, PathBuf};

use crate::error::{Error, Result, io};

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
}

impl Workspace {
    pub fn discover() -> Result<Self> {
        let cwd = std::env::current_dir().map_err(io("."))?;
        Self::discover_from(&cwd)
    }

    pub fn discover_from(start: &Path) -> Result<Self> {
        if let Ok(explicit) = std::env::var("WRE_ROOT") {
            return Ok(Self { root: PathBuf::from(explicit) });
        }

        let mut cursor = Some(start);
        while let Some(dir) = cursor {
            if dir.join("wre.toml").is_file() || dir.join(".git").exists() {
                return Ok(Self { root: dir.to_path_buf() });
            }
            cursor = dir.parent();
        }

        Err(Error::NoWorkspaceRoot(start.to_path_buf()))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn artifacts(&self) -> PathBuf {
        self.root.join("artifacts")
    }

    pub fn captures(&self) -> PathBuf {
        self.root.join("captures")
    }

    pub fn reference(&self) -> PathBuf {
        self.root.join("reference")
    }

    pub fn targets(&self) -> PathBuf {
        self.root.join("targets")
    }

    pub fn profiles(&self) -> PathBuf {
        self.root.join("profiles")
    }

    pub fn baselines(&self) -> PathBuf {
        self.reference().join("baselines")
    }

    pub fn cache(&self) -> PathBuf {
        self.artifacts().join("cache")
    }

    pub fn chrome_profiles(&self) -> PathBuf {
        self.artifacts().join("chrome")
    }

    pub fn scratch(&self) -> PathBuf {
        self.artifacts().join("scratch")
    }

    pub fn artifact(&self, kind: &str) -> PathBuf {
        self.artifacts().join(kind)
    }

    pub fn target_dir(&self, target: &str) -> PathBuf {
        self.targets().join(target)
    }

    pub fn capture_dir(&self, name: &str) -> PathBuf {
        self.captures().join(name)
    }

    pub fn ensure(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path).map_err(io(path))
    }
}

pub fn safe_name(value: &str) -> String {
    let trimmed = value
        .trim_start_matches("https://")
        .trim_start_matches("http://");

    let mut out = String::with_capacity(trimmed.len().min(120));
    let mut last_underscore = false;

    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
            out.push(ch);
            last_underscore = false;
        } else if !last_underscore {
            out.push('_');
            last_underscore = true;
        }

        if out.len() >= 120 {
            break;
        }
    }

    if out.is_empty() { "unnamed".to_string() } else { out }
}

pub fn stamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string()
}

pub fn day() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}
