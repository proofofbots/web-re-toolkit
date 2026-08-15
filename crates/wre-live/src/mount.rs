use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};
use wre_js::pipeline::SourceKind;
use wre_js::surface::{SignatureRule, detect_roles};

use crate::realm::{FunctionHandle, MountReport, Realm, RealmOptions};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcePatch {
    pub find: String,
    pub replace: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub limit: usize,
    #[serde(default)]
    pub required: bool,
}

impl SourcePatch {
    pub fn literal(find: impl Into<String>, replace: impl Into<String>) -> Self {
        Self {
            find: find.into(),
            replace: replace.into(),
            regex: false,
            limit: 0,
            required: true,
        }
    }

    pub fn pattern(find: impl Into<String>, replace: impl Into<String>) -> Self {
        Self {
            find: find.into(),
            replace: replace.into(),
            regex: true,
            limit: 0,
            required: true,
        }
    }

    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    pub fn once(mut self) -> Self {
        self.limit = 1;
        self
    }
}

pub fn apply_patches(source: &str, patches: &[SourcePatch]) -> Result<(String, usize)> {
    let mut current = source.to_string();
    let mut applied = 0usize;

    for patch in patches {
        let before = current.clone();

        current = if patch.regex {
            let regex = regex::Regex::new(&patch.find)
                .map_err(|error| Error::msg(format!("patch pattern {}: {error}", patch.find)))?;
            if patch.limit == 0 {
                regex.replace_all(&current, patch.replace.as_str()).into_owned()
            } else {
                regex
                    .replacen(&current, patch.limit, patch.replace.as_str())
                    .into_owned()
            }
        } else if patch.limit == 0 {
            current.replace(&patch.find, &patch.replace)
        } else {
            current.replacen(&patch.find, &patch.replace, patch.limit)
        };

        if current == before {
            if patch.required {
                return Err(Error::msg(format!(
                    "patch did not match anything: {}",
                    truncate(&patch.find)
                )));
            }
        } else {
            applied += 1;
        }
    }

    Ok((current, applied))
}

fn truncate(text: &str) -> String {
    if text.len() <= 80 {
        text.to_string()
    } else {
        format!("{}…", &text[..80])
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MountPlan {
    #[serde(default)]
    pub prelude: Vec<String>,
    #[serde(default)]
    pub patches: Vec<SourcePatch>,
    #[serde(default)]
    pub exports: BTreeMap<String, String>,
    #[serde(default)]
    pub after: Vec<String>,
    #[serde(default)]
    pub signatures: Vec<SignatureRule>,
    #[serde(default)]
    pub source_kind: SourceKind,
    #[serde(default)]
    pub tolerate_throw: bool,
}

impl MountPlan {
    pub fn with_export(mut self, role: &str, expression: &str) -> Self {
        self.exports.insert(role.to_string(), expression.to_string());
        self
    }

    pub fn with_patch(mut self, patch: SourcePatch) -> Self {
        self.patches.push(patch);
        self
    }

    pub fn with_signature(mut self, role: &str, pattern: &str) -> Self {
        self.signatures.push(SignatureRule {
            role: role.to_string(),
            pattern: pattern.to_string(),
            params: None,
        });
        self
    }
}

pub struct Mount {
    pub realm: Realm,
    pub handles: BTreeMap<String, FunctionHandle>,
    pub report: MountReport,
    pub source: String,
}

impl Mount {
    pub fn handle(&self, role: &str) -> Result<&FunctionHandle> {
        self.handles
            .get(role)
            .ok_or_else(|| Error::msg(format!("role {role} was not captured")))
    }

    pub fn call(&mut self, role: &str, args: &[serde_json::Value]) -> Result<serde_json::Value> {
        let handle = self
            .handles
            .get(role)
            .cloned()
            .ok_or_else(|| Error::msg(format!("role {role} was not captured")))?;
        self.realm.call(&handle, args)
    }

    pub fn roles(&self) -> Vec<String> {
        self.handles.keys().cloned().collect()
    }
}

pub fn mount(source: &str, plan: &MountPlan, options: RealmOptions) -> Result<Mount> {
    let (patched, applied) = apply_patches(source, &plan.patches)?;

    let detected = if plan.signatures.is_empty() {
        BTreeMap::new()
    } else {
        let roles = detect_roles(&patched, plan.source_kind, &plan.signatures)?;
        if !roles.unmatched.is_empty() {
            tracing::debug!("signatures without a match: {:?}", roles.unmatched);
        }
        roles.roles
    };

    let prepared = if detected.is_empty() {
        patched
    } else {
        let block = role_export_block(&detected);
        let (directives, body) = split_directives(&patched);
        format!("{directives}{block}{body}\n{block}")
    };

    finish(prepared, plan, options, detected, applied)
}

fn role_export_block(detected: &BTreeMap<String, String>) -> String {
    let mut out = String::from("\n;globalThis.__wreRoles = globalThis.__wreRoles || {};\n");

    for (role, name) in detected {
        out.push_str(&format!(
            ";try {{ if (typeof {name} !== 'undefined' && {name} !== undefined) globalThis.__wreRoles[{}] = {name}; }} catch (error) {{}}\n",
            quote_key(role)
        ));
    }

    out
}

fn split_directives(source: &str) -> (String, String) {
    let mut cursor = 0usize;

    for line in source.lines() {
        let trimmed = line.trim();
        let is_directive = trimmed.starts_with("\"use ") || trimmed.starts_with("'use ");
        if trimmed.is_empty() || is_directive {
            cursor += line.len() + 1;
            if !is_directive && cursor >= source.len() {
                break;
            }
            continue;
        }
        break;
    }

    let cursor = cursor.min(source.len());
    if !source.is_char_boundary(cursor) {
        return (String::new(), source.to_string());
    }

    (source[..cursor].to_string(), source[cursor..].to_string())
}

fn quote_key(key: &str) -> String {
    serde_json::to_string(key).unwrap_or_else(|_| format!("\"{key}\""))
}

fn finish(
    prepared: String,
    plan: &MountPlan,
    options: RealmOptions,
    detected: BTreeMap<String, String>,
    applied: usize,
) -> Result<Mount> {
    let mut realm = Realm::new(options)?;

    for source in &plan.prelude {
        realm.eval_unit(source, "wre:mount-prelude")?;
    }

    let bytes = prepared.len();

    match realm.eval_unit(&prepared, "wre:target") {
        Ok(()) => {}
        Err(error) => {
            if !plan.tolerate_throw {
                return Err(error);
            }
            tracing::debug!("target threw during mount, continuing: {error}");
        }
    }

    for source in &plan.after {
        realm.eval_unit(source, "wre:mount-after")?;
    }

    let mut handles = BTreeMap::new();
    let mut roles = BTreeMap::new();

    for role in detected.keys() {
        let expression = format!("__wreRoles[{}]", quote_key(role));
        match realm.capture(role, &expression) {
            Ok(handle) => {
                handles.insert(role.clone(), handle);
                roles.insert(role.clone(), true);
            }
            Err(error) => {
                tracing::debug!("role {role} did not capture: {error}");
                roles.insert(role.clone(), false);
            }
        }
    }

    for (role, expression) in &plan.exports {
        match realm.capture(role, expression) {
            Ok(handle) => {
                handles.insert(role.clone(), handle);
                roles.insert(role.clone(), true);
            }
            Err(error) => {
                tracing::debug!("export {role} did not capture: {error}");
                roles.insert(role.clone(), false);
            }
        }
    }

    let records = realm.records().unwrap_or_default();

    Ok(Mount {
        realm,
        handles,
        report: MountReport { roles, records, bytes, patched: applied },
        source: prepared,
    })
}
