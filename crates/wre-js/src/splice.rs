use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
    #[serde(default)]
    pub reason: String,
}

impl Edit {
    pub fn new(start: usize, end: usize, text: impl Into<String>) -> Self {
        Self { start, end, text: text.into(), reason: String::new() }
    }

    pub fn because(mut self, reason: impl Into<String>) -> Self {
        self.reason = reason.into();
        self
    }

    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0 && self.text.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct EditLog {
    edits: Vec<Edit>,
}

impl EditLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, edit: Edit) {
        self.edits.push(edit);
    }

    pub fn replace(&mut self, start: usize, end: usize, text: impl Into<String>) {
        self.push(Edit::new(start, end, text));
    }

    pub fn insert(&mut self, at: usize, text: impl Into<String>) {
        self.push(Edit::new(at, at, text));
    }

    pub fn remove(&mut self, start: usize, end: usize) {
        self.push(Edit::new(start, end, ""));
    }

    pub fn len(&self) -> usize {
        self.edits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    pub fn edits(&self) -> &[Edit] {
        &self.edits
    }

    pub fn apply(&self, source: &str) -> Result<String> {
        let mut sorted = self.edits.clone();
        sorted.sort_by_key(|edit| (edit.start, edit.end));

        let mut out = String::with_capacity(source.len());
        let mut cursor = 0usize;

        for edit in &sorted {
            if edit.start < cursor {
                return Err(Error::msg(format!(
                    "edit at {}..{} overlaps an earlier edit ending at {cursor}",
                    edit.start, edit.end
                )));
            }

            if edit.end > source.len() {
                return Err(Error::msg(format!(
                    "edit at {}..{} runs past the {} byte source",
                    edit.start,
                    edit.end,
                    source.len()
                )));
            }

            if !source.is_char_boundary(edit.start) || !source.is_char_boundary(edit.end) {
                return Err(Error::msg(format!(
                    "edit at {}..{} does not fall on character boundaries",
                    edit.start, edit.end
                )));
            }

            out.push_str(&source[cursor..edit.start]);
            out.push_str(&edit.text);
            cursor = edit.end;
        }

        out.push_str(&source[cursor..]);
        Ok(out)
    }

    pub fn size_delta(&self) -> isize {
        self.edits
            .iter()
            .map(|edit| edit.text.len() as isize - edit.len() as isize)
            .sum()
    }

    pub fn merge(&mut self, other: EditLog) {
        self.edits.extend(other.edits);
    }
}

pub fn find_all(source: &str, needle: &str) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut cursor = 0usize;

    while let Some(index) = source[cursor..].find(needle) {
        let absolute = cursor + index;
        out.push(absolute);
        cursor = absolute + needle.len();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_edits_in_order() {
        let mut log = EditLog::new();
        log.replace(0, 3, "XYZ");
        log.insert(6, "!");
        log.remove(9, 12);

        let out = log.apply("abcdefghijkl").unwrap();
        assert_eq!(out, "XYZdef!ghi");
    }

    #[test]
    fn rejects_overlapping_edits() {
        let mut log = EditLog::new();
        log.replace(0, 5, "a");
        log.replace(3, 8, "b");
        assert!(log.apply("0123456789").is_err());
    }

    #[test]
    fn finds_every_occurrence() {
        assert_eq!(find_all("aXbXc", "X"), vec![1, 3]);
        assert_eq!(find_all("aaa", "aa"), vec![0]);
    }
}
