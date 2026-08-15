use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Segment {
    Key(String),
    Index(usize),
    AnyKey,
    AnyDepth,
}

impl fmt::Display for Segment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Segment::Key(key) => {
                if is_bare(key) {
                    write!(formatter, "{key}")
                } else {
                    write!(formatter, "{}", quote(key))
                }
            }
            Segment::Index(index) => write!(formatter, "[{index}]"),
            Segment::AnyKey => write!(formatter, "*"),
            Segment::AnyDepth => write!(formatter, "**"),
        }
    }
}

fn is_bare(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '$')
        && key != "*"
        && key != "**"
}

fn quote(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 2);
    out.push('"');
    for ch in key.chars() {
        if ch == '"' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Address {
    pub segments: Vec<Segment>,
}

impl Address {
    pub fn root() -> Self {
        Self { segments: Vec::new() }
    }

    pub fn is_root(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.segments.push(Segment::Key(key.into()));
        self
    }

    pub fn index(mut self, index: usize) -> Self {
        self.segments.push(Segment::Index(index));
        self
    }

    pub fn parent(&self) -> Option<Address> {
        if self.segments.is_empty() {
            return None;
        }
        let mut segments = self.segments.clone();
        segments.pop();
        Some(Address { segments })
    }

    pub fn last_key(&self) -> Option<&str> {
        match self.segments.last() {
            Some(Segment::Key(key)) => Some(key),
            _ => None,
        }
    }

    pub fn has_wildcard(&self) -> bool {
        self.segments
            .iter()
            .any(|segment| matches!(segment, Segment::AnyKey | Segment::AnyDepth))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let mut segments = Vec::new();
        let bytes: Vec<char> = text.chars().collect();
        let mut cursor = 0usize;
        let mut expect_separator = false;

        while cursor < bytes.len() {
            match bytes[cursor] {
                '.' => {
                    if !expect_separator {
                        return Err(Error::BadAddress(text.to_string()));
                    }
                    expect_separator = false;
                    cursor += 1;
                }
                '[' => {
                    let close = bytes[cursor..]
                        .iter()
                        .position(|ch| *ch == ']')
                        .ok_or_else(|| Error::BadAddress(text.to_string()))?
                        + cursor;
                    let inner: String = bytes[cursor + 1..close].iter().collect();
                    let index = inner
                        .parse::<usize>()
                        .map_err(|_| Error::BadAddress(text.to_string()))?;
                    segments.push(Segment::Index(index));
                    cursor = close + 1;
                    expect_separator = true;
                }
                '"' => {
                    let mut key = String::new();
                    cursor += 1;
                    while cursor < bytes.len() && bytes[cursor] != '"' {
                        if bytes[cursor] == '\\' && cursor + 1 < bytes.len() {
                            cursor += 1;
                        }
                        key.push(bytes[cursor]);
                        cursor += 1;
                    }
                    if cursor >= bytes.len() {
                        return Err(Error::BadAddress(text.to_string()));
                    }
                    cursor += 1;
                    segments.push(Segment::Key(key));
                    expect_separator = true;
                }
                _ => {
                    let start = cursor;
                    while cursor < bytes.len() && bytes[cursor] != '.' && bytes[cursor] != '[' {
                        cursor += 1;
                    }
                    let raw: String = bytes[start..cursor].iter().collect();
                    if raw.is_empty() {
                        return Err(Error::BadAddress(text.to_string()));
                    }
                    segments.push(match raw.as_str() {
                        "*" => Segment::AnyKey,
                        "**" => Segment::AnyDepth,
                        _ => Segment::Key(raw),
                    });
                    expect_separator = true;
                }
            }
        }

        Ok(Self { segments })
    }

    pub fn get<'v>(&self, value: &'v Value) -> Option<&'v Value> {
        let mut cursor = value;
        for segment in &self.segments {
            cursor = match segment {
                Segment::Key(key) => cursor.get(key)?,
                Segment::Index(index) => cursor.get(index)?,
                Segment::AnyKey | Segment::AnyDepth => return None,
            };
        }
        Some(cursor)
    }

    pub fn get_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value> {
        let mut cursor = value;
        for segment in &self.segments {
            cursor = match segment {
                Segment::Key(key) => cursor.get_mut(key)?,
                Segment::Index(index) => cursor.get_mut(index)?,
                Segment::AnyKey | Segment::AnyDepth => return None,
            };
        }
        Some(cursor)
    }

    pub fn set(&self, root: &mut Value, replacement: Value) -> Result<()> {
        if self.segments.is_empty() {
            *root = replacement;
            return Ok(());
        }

        let mut cursor = root;
        for segment in &self.segments[..self.segments.len() - 1] {
            cursor = match segment {
                Segment::Key(key) => {
                    if !cursor.is_object() {
                        *cursor = Value::Object(Map::new());
                    }
                    cursor
                        .as_object_mut()
                        .expect("object")
                        .entry(key.clone())
                        .or_insert(Value::Null)
                }
                Segment::Index(index) => {
                    if !cursor.is_array() {
                        *cursor = Value::Array(Vec::new());
                    }
                    let array = cursor.as_array_mut().expect("array");
                    if array.len() <= *index {
                        array.resize(index + 1, Value::Null);
                    }
                    &mut array[*index]
                }
                Segment::AnyKey | Segment::AnyDepth => {
                    return Err(Error::BadAddress(self.to_string()));
                }
            };
        }

        match self.segments.last().expect("non empty") {
            Segment::Key(key) => {
                if !cursor.is_object() {
                    *cursor = Value::Object(Map::new());
                }
                cursor
                    .as_object_mut()
                    .expect("object")
                    .insert(key.clone(), replacement);
            }
            Segment::Index(index) => {
                if !cursor.is_array() {
                    *cursor = Value::Array(Vec::new());
                }
                let array = cursor.as_array_mut().expect("array");
                if array.len() <= *index {
                    array.resize(index + 1, Value::Null);
                }
                array[*index] = replacement;
            }
            Segment::AnyKey | Segment::AnyDepth => {
                return Err(Error::BadAddress(self.to_string()));
            }
        }

        Ok(())
    }

    pub fn remove(&self, root: &mut Value) -> Option<Value> {
        let parent = self.parent()?;
        let holder = parent.get_mut(root)?;
        match self.segments.last()? {
            Segment::Key(key) => holder.as_object_mut()?.shift_remove(key),
            Segment::Index(index) => {
                let array = holder.as_array_mut()?;
                if *index < array.len() { Some(array.remove(*index)) } else { None }
            }
            _ => None,
        }
    }

    pub fn matches(&self, concrete: &Address) -> bool {
        matches_from(&self.segments, &concrete.segments)
    }
}

fn matches_from(pattern: &[Segment], concrete: &[Segment]) -> bool {
    match pattern.first() {
        None => concrete.is_empty(),
        Some(Segment::AnyDepth) => {
            for skip in 0..=concrete.len() {
                if matches_from(&pattern[1..], &concrete[skip..]) {
                    return true;
                }
            }
            false
        }
        Some(head) => {
            let Some(actual) = concrete.first() else {
                return false;
            };
            let ok = match head {
                Segment::AnyKey => true,
                other => other == actual,
            };
            ok && matches_from(&pattern[1..], &concrete[1..])
        }
    }
}

impl fmt::Display for Address {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for segment in &self.segments {
            match segment {
                Segment::Index(_) => write!(formatter, "{segment}")?,
                _ => {
                    if !first {
                        write!(formatter, ".")?;
                    }
                    write!(formatter, "{segment}")?;
                }
            }
            first = false;
        }
        Ok(())
    }
}

impl Serialize for Address {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Address {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Address::parse(&text).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for Address {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        Address::parse(text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeafKind {
    Null,
    Bool,
    Number,
    String,
    EmptyArray,
    EmptyObject,
}

pub fn leaves(root: &Value) -> Vec<(Address, &Value)> {
    let mut out = Vec::new();
    collect(root, Address::root(), &mut out);
    out
}

fn collect<'v>(value: &'v Value, at: Address, out: &mut Vec<(Address, &'v Value)>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, child) in map {
                collect(child, at.clone().key(key.clone()), out);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for (index, child) in items.iter().enumerate() {
                collect(child, at.clone().index(index), out);
            }
        }
        _ => out.push((at, value)),
    }
}

pub fn leaf_kind(value: &Value) -> LeafKind {
    match value {
        Value::Null => LeafKind::Null,
        Value::Bool(_) => LeafKind::Bool,
        Value::Number(_) => LeafKind::Number,
        Value::String(_) => LeafKind::String,
        Value::Array(_) => LeafKind::EmptyArray,
        Value::Object(_) => LeafKind::EmptyObject,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mixed_forms() {
        let address = Address::parse("-115[9]").unwrap();
        assert_eq!(address.segments, vec![Segment::Key("-115".into()), Segment::Index(9)]);
        assert_eq!(address.to_string(), "-115[9]");

        let address = Address::parse("s7.v").unwrap();
        assert_eq!(address.to_string(), "s7.v");

        let address = Address::parse("\"weird key\".a[2]").unwrap();
        assert_eq!(address.segments.len(), 3);
    }

    #[test]
    fn sets_and_reads_back() {
        let mut value = Value::Null;
        Address::parse("a.b[1].c")
            .unwrap()
            .set(&mut value, Value::from(7))
            .unwrap();
        assert_eq!(
            Address::parse("a.b[1].c").unwrap().get(&value),
            Some(&Value::from(7))
        );
    }

    #[test]
    fn wildcards_match() {
        let pattern = Address::parse("**.c").unwrap();
        let concrete = Address::parse("a.b.c").unwrap();
        assert!(pattern.matches(&concrete));

        let pattern = Address::parse("a.*.c").unwrap();
        assert!(pattern.matches(&concrete));
        assert!(!pattern.matches(&Address::parse("a.b.d").unwrap()));
    }
}
