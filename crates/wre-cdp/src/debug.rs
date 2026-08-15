use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use wre_core::error::{Error, Result};

use crate::session::Session;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

pub fn position_of(source: &str, offset: usize) -> Position {
    let mut line = 0u32;
    let mut line_start = 0usize;

    for (index, byte) in source.as_bytes().iter().enumerate() {
        if index >= offset {
            break;
        }
        if *byte == b'\n' {
            line += 1;
            line_start = index + 1;
        }
    }

    Position { line, column: (offset.saturating_sub(line_start)) as u32 }
}

pub fn offset_of_pattern(source: &str, pattern: &str, occurrence: usize) -> Result<usize> {
    let mut found = Vec::new();
    let mut cursor = 0usize;

    while let Some(index) = source[cursor..].find(pattern) {
        found.push(cursor + index);
        cursor += index + pattern.len().max(1);
    }

    if found.is_empty() {
        return Err(Error::msg(format!("pattern {pattern} not present in source")));
    }

    let chosen = found.get(occurrence).copied().unwrap_or_else(|| found[found.len() - 1]);
    Ok(chosen)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breakpoint {
    pub id: String,
    pub url_regex: String,
    pub position: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallFrame {
    pub call_frame_id: String,
    pub function_name: String,
    pub url: String,
    pub position: Position,
    pub scopes: Vec<ScopeRef>,
    pub this_object: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeRef {
    pub kind: String,
    pub name: Option<String>,
    pub object_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyDump {
    pub name: String,
    pub kind: String,
    pub subtype: Option<String>,
    pub value: Option<Value>,
    pub description: Option<String>,
    pub object_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PauseReport {
    pub reason: String,
    pub frames: Vec<CallFrame>,
    pub hit_breakpoints: Vec<String>,
}

pub struct Debugger {
    session: Session,
}

impl Debugger {
    pub async fn attach(session: &Session) -> Result<Self> {
        session.send("Debugger.enable", json!({})).await?;
        session
            .send("Runtime.enable", json!({}))
            .await
            .or_else(|_| Ok::<Value, Error>(Value::Null))?;
        Ok(Self { session: session.clone() })
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub async fn blackbox(&self, patterns: &[&str]) -> Result<()> {
        self.session
            .send(
                "Debugger.setBlackboxPatterns",
                json!({ "patterns": patterns }),
            )
            .await?;
        Ok(())
    }

    pub async fn set_breakpoint(
        &self,
        url_regex: &str,
        position: Position,
        condition: Option<&str>,
    ) -> Result<Breakpoint> {
        let mut params = json!({
            "urlRegex": url_regex,
            "lineNumber": position.line,
            "columnNumber": position.column,
        });

        if let Some(condition) = condition {
            params["condition"] = Value::String(condition.to_string());
        }

        let result = self
            .session
            .send("Debugger.setBreakpointByUrl", params)
            .await?;

        let id = result
            .get("breakpointId")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::msg("setBreakpointByUrl returned no id"))?
            .to_string();

        Ok(Breakpoint { id, url_regex: url_regex.to_string(), position })
    }

    pub async fn set_breakpoint_at_pattern(
        &self,
        url_regex: &str,
        source: &str,
        pattern: &str,
        occurrence: usize,
        condition: Option<&str>,
    ) -> Result<Breakpoint> {
        let offset = offset_of_pattern(source, pattern, occurrence)?;
        let position = position_of(source, offset);
        self.set_breakpoint(url_regex, position, condition).await
    }

    pub async fn remove_breakpoint(&self, id: &str) -> Result<()> {
        self.session
            .send("Debugger.removeBreakpoint", json!({ "breakpointId": id }))
            .await?;
        Ok(())
    }

    pub async fn pause_on_exceptions(&self, state: &str) -> Result<()> {
        self.session
            .send("Debugger.setPauseOnExceptions", json!({ "state": state }))
            .await?;
        Ok(())
    }

    pub async fn wait_for_pause(&self, timeout: Duration) -> Result<PauseReport> {
        let event = self.session.wait_for_event("Debugger.paused", timeout).await?;
        Ok(parse_pause(&event.params))
    }

    pub async fn resume(&self) -> Result<()> {
        self.session.send("Debugger.resume", json!({})).await?;
        Ok(())
    }

    pub async fn step_over(&self) -> Result<()> {
        self.session.send("Debugger.stepOver", json!({})).await?;
        Ok(())
    }

    pub async fn step_into(&self) -> Result<()> {
        self.session.send("Debugger.stepInto", json!({})).await?;
        Ok(())
    }

    pub async fn step_out(&self) -> Result<()> {
        self.session.send("Debugger.stepOut", json!({})).await?;
        Ok(())
    }

    pub async fn evaluate_on_frame(&self, frame_id: &str, expression: &str) -> Result<Value> {
        let result = self
            .session
            .send(
                "Debugger.evaluateOnCallFrame",
                json!({
                    "callFrameId": frame_id,
                    "expression": expression,
                    "returnByValue": true,
                    "silent": true,
                }),
            )
            .await?;

        Ok(result
            .get("result")
            .and_then(|value| value.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    pub async fn properties(&self, object_id: &str, limit: usize) -> Result<Vec<PropertyDump>> {
        let result = self
            .session
            .send(
                "Runtime.getProperties",
                json!({
                    "objectId": object_id,
                    "ownProperties": true,
                    "accessorPropertiesOnly": false,
                    "generatePreview": false,
                }),
            )
            .await?;

        let entries = result
            .get("result")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        Ok(entries
            .into_iter()
            .take(limit)
            .map(|entry| {
                let value = entry.get("value");
                PropertyDump {
                    name: entry
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    kind: value
                        .and_then(|value| value.get("type"))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string(),
                    subtype: value
                        .and_then(|value| value.get("subtype"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    value: value.and_then(|value| value.get("value")).cloned(),
                    description: value
                        .and_then(|value| value.get("description"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    object_id: value
                        .and_then(|value| value.get("objectId"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                }
            })
            .collect())
    }

    pub async fn dump_scopes(&self, frame: &CallFrame, limit: usize) -> Result<Vec<ScopeDump>> {
        let mut out = Vec::new();

        for scope in &frame.scopes {
            let Some(object_id) = &scope.object_id else {
                continue;
            };
            if scope.kind == "global" {
                continue;
            }

            let properties = self.properties(object_id, limit).await.unwrap_or_default();
            out.push(ScopeDump {
                kind: scope.kind.clone(),
                name: scope.name.clone(),
                properties,
            });
        }

        Ok(out)
    }

    pub async fn script_source(&self, script_id: &str) -> Result<String> {
        let result = self
            .session
            .send("Debugger.getScriptSource", json!({ "scriptId": script_id }))
            .await?;

        Ok(result
            .get("scriptSource")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    pub async fn search_in_scripts(&self, script_id: &str, query: &str) -> Result<Vec<(u32, String)>> {
        let result = self
            .session
            .send(
                "Debugger.searchInContent",
                json!({ "scriptId": script_id, "query": query, "caseSensitive": true }),
            )
            .await?;

        Ok(result
            .get("result")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|entry| {
                Some((
                    entry.get("lineNumber")?.as_u64()? as u32,
                    entry.get("lineContent")?.as_str()?.to_string(),
                ))
            })
            .collect())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeDump {
    pub kind: String,
    pub name: Option<String>,
    pub properties: Vec<PropertyDump>,
}

pub fn parse_pause(params: &Value) -> PauseReport {
    let frames = params
        .get("callFrames")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|frame| CallFrame {
            call_frame_id: frame
                .get("callFrameId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            function_name: frame
                .get("functionName")
                .and_then(Value::as_str)
                .unwrap_or("<anonymous>")
                .to_string(),
            url: frame
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            position: Position {
                line: frame
                    .get("location")
                    .and_then(|value| value.get("lineNumber"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                column: frame
                    .get("location")
                    .and_then(|value| value.get("columnNumber"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
            },
            scopes: frame
                .get("scopeChain")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|scope| ScopeRef {
                    kind: scope
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string(),
                    name: scope.get("name").and_then(Value::as_str).map(str::to_string),
                    object_id: scope
                        .get("object")
                        .and_then(|value| value.get("objectId"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                })
                .collect(),
            this_object: frame
                .get("this")
                .and_then(|value| value.get("objectId"))
                .and_then(Value::as_str)
                .map(str::to_string),
        })
        .collect();

    PauseReport {
        reason: params
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("other")
            .to_string(),
        frames,
        hit_breakpoints: params
            .get("hitBreakpoints")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_offsets_to_positions() {
        let source = "aaa\nbbbb\ncc";
        assert_eq!(position_of(source, 0), Position { line: 0, column: 0 });
        assert_eq!(position_of(source, 4), Position { line: 1, column: 0 });
        assert_eq!(position_of(source, 6), Position { line: 1, column: 2 });
        assert_eq!(position_of(source, 9), Position { line: 2, column: 0 });
    }

    #[test]
    fn finds_nth_pattern() {
        let source = "x=1; needle; y=2; needle; z=3;";
        assert_eq!(offset_of_pattern(source, "needle", 0).unwrap(), 5);
        assert_eq!(offset_of_pattern(source, "needle", 1).unwrap(), 18);
        assert!(offset_of_pattern(source, "missing", 0).is_err());
    }
}
