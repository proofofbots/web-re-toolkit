use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result, io, json};
use wre_core::paths::safe_name;

use crate::profile::Profile;

pub const BUILTIN_ID: &str = "builtin-desktop-chrome";

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Origin {
    #[serde(default)]
    pub user_agent: String,
    #[serde(default)]
    pub font_probe: String,
    #[serde(default)]
    pub page: String,
    #[serde(default)]
    pub client: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub captured_at: String,
    #[serde(default)]
    pub origin: Origin,
    pub profile: Profile,
}

impl Record {
    pub fn builtin() -> Self {
        let profile = Profile::desktop_chrome();
        let user_agent = profile
            .property("Navigator", "userAgent")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();

        Self {
            id: BUILTIN_ID.to_string(),
            label: "MacBook Pro M1 Pro, Chrome 140, built in".to_string(),
            notes: "Compiled into wre-sandbox. Captured from a real machine, kept so the sandbox \
                    runs before anyone has captured a profile of their own."
                .to_string(),
            captured_at: String::new(),
            origin: Origin { user_agent, ..Origin::default() },
            profile,
        }
    }

    pub fn is_builtin(&self) -> bool {
        self.id == BUILTIN_ID
    }

    pub fn summary(&self) -> String {
        let agent = if self.origin.user_agent.is_empty() {
            self.profile
                .property("Navigator", "userAgent")
                .and_then(|value| value.as_str())
                .unwrap_or("no user agent")
        } else {
            &self.origin.user_agent
        };

        format!("{} ({agent})", if self.label.is_empty() { &self.id } else { &self.label })
    }
}

#[derive(Debug, Clone)]
pub struct Library {
    dir: PathBuf,
    records: Vec<Record>,
}

impl Library {
    pub fn load(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        let mut records = Vec::new();

        if dir.is_dir() {
            let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
                .map_err(io(&dir))?
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
                .collect();
            paths.sort();

            for path in paths {
                records.push(read(&path)?);
            }
        }

        Ok(Self { dir, records })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn records(&self) -> &[Record] {
        &self.records
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn ids(&self) -> Vec<String> {
        self.records.iter().map(|record| record.id.clone()).collect()
    }

    pub fn get(&self, id: &str) -> Option<&Record> {
        self.records.iter().find(|record| record.id == id)
    }

    pub fn first(&self) -> Option<&Record> {
        self.records.first()
    }

    pub fn random(&self) -> Option<&Record> {
        if self.records.is_empty() {
            return None;
        }

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.subsec_nanos() as usize)
            .unwrap_or(0);

        self.records.get(nanos % self.records.len())
    }

    pub fn resolve(&self, id: Option<&str>, random: bool) -> Result<Record> {
        if let Some(id) = id {
            if id == BUILTIN_ID {
                return Ok(Record::builtin());
            }

            return self.get(id).cloned().ok_or_else(|| {
                let known = if self.records.is_empty() {
                    "the library is empty, run `wre sandbox capture`".to_string()
                } else {
                    format!("known ids: {}", self.ids().join(", "))
                };
                Error::msg(format!("no profile {id} in {}, {known}", self.dir.display()))
            });
        }

        if random {
            return self
                .random()
                .cloned()
                .ok_or_else(|| Error::msg("--random needs a captured profile, run `wre sandbox capture`"));
        }

        Ok(self.first().cloned().unwrap_or_else(Record::builtin))
    }

    pub fn path_for(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    pub fn store(&mut self, mut record: Record, force: bool) -> Result<PathBuf> {
        record.id = safe_name(&record.id);
        if record.id.is_empty() || record.id == BUILTIN_ID {
            return Err(Error::msg(format!("{} is not a usable profile id", record.id)));
        }

        let path = self.path_for(&record.id);
        if path.exists() && !force {
            record.id = free_id(&self.dir, &record.id);
        }

        let path = self.path_for(&record.id);
        std::fs::create_dir_all(&self.dir).map_err(io(&self.dir))?;

        let text = serde_json::to_string_pretty(&record)
            .map_err(|error| Error::msg(format!("profile did not serialise: {error}")))?;
        std::fs::write(&path, text).map_err(io(&path))?;

        self.records.retain(|entry| entry.id != record.id);
        self.records.push(record);
        self.records.sort_by(|left, right| left.id.cmp(&right.id));

        Ok(path)
    }
}

fn free_id(dir: &Path, id: &str) -> String {
    for suffix in 2..1000u32 {
        let candidate = format!("{id}-{suffix}");
        if !dir.join(format!("{candidate}.json")).exists() {
            return candidate;
        }
    }

    format!("{id}-{}", stamp_suffix())
}

fn stamp_suffix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

fn read(path: &Path) -> Result<Record> {
    let text = std::fs::read_to_string(path).map_err(io(path))?;
    let mut record: Record = serde_json::from_str(&text).map_err(json(path))?;

    if record.id.is_empty() {
        record.id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("unnamed")
            .to_string();
    }

    record.profile.validate()?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wre-sandbox-{name}-{}", stamp_suffix()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn an_empty_library_resolves_to_the_builtin() {
        let library = Library::load(temp_dir("empty")).unwrap();

        assert!(library.is_empty());
        assert!(library.resolve(None, false).unwrap().is_builtin());
        assert!(library.resolve(None, true).is_err());
    }

    #[test]
    fn a_stored_profile_is_read_back_and_wins_over_the_builtin() {
        let dir = temp_dir("store");
        let mut library = Library::load(&dir).unwrap();

        let record = Record {
            id: "pixel 8/chrome".to_string(),
            label: "Pixel 8".to_string(),
            notes: String::new(),
            captured_at: "2026-08-16T00:00:00Z".to_string(),
            origin: Origin::default(),
            profile: Profile::desktop_chrome(),
        };

        let path = library.store(record, false).unwrap();
        assert!(path.exists());

        let reloaded = Library::load(&dir).unwrap();
        assert_eq!(reloaded.ids(), vec!["pixel_8_chrome".to_string()]);
        assert_eq!(reloaded.resolve(None, false).unwrap().label, "Pixel 8");
        assert!(reloaded.resolve(Some(BUILTIN_ID), false).unwrap().is_builtin());
        assert!(reloaded.resolve(Some("nothing"), false).is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn storing_the_same_id_twice_keeps_both_unless_forced() {
        let dir = temp_dir("collide");
        let mut library = Library::load(&dir).unwrap();

        let record = Record {
            id: "iphone-15".to_string(),
            label: String::new(),
            notes: String::new(),
            captured_at: String::new(),
            origin: Origin::default(),
            profile: Profile::desktop_chrome(),
        };

        library.store(record.clone(), false).unwrap();
        let second = library.store(record.clone(), false).unwrap();
        assert!(second.ends_with("iphone-15-2.json"));

        let third = library.store(record, true).unwrap();
        assert!(third.ends_with("iphone-15.json"));
        assert_eq!(library.records().len(), 2);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
