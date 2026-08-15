use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};
use wre_crypto::checksum::{Checksum, murmur3_skipping_whitespace};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    #[default]
    ExcludingMarker,
    AfterMarker,
    MarkerZeroed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Guard {
    pub marker: String,
    #[serde(default)]
    pub checksum: Checksum,
    #[serde(default)]
    pub seed: u32,
    #[serde(default)]
    pub skip_whitespace: bool,
    #[serde(default)]
    pub scope: Scope,
}

impl Guard {
    pub fn new(marker: impl Into<String>) -> Self {
        Self {
            marker: marker.into(),
            checksum: Checksum::Murmur3,
            seed: 0,
            skip_whitespace: false,
            scope: Scope::default(),
        }
    }

    pub fn seeded(mut self, seed: u32) -> Self {
        self.seed = seed;
        self
    }

    pub fn skipping_whitespace(mut self) -> Self {
        self.skip_whitespace = true;
        self
    }

    pub fn over(mut self, scope: Scope) -> Self {
        self.scope = scope;
        self
    }

    fn compiled(&self) -> Result<regex::Regex> {
        let regex = regex::Regex::new(&self.marker)
            .map_err(|error| Error::msg(format!("integrity marker does not compile: {error}")))?;

        if regex.captures_len() < 2 {
            return Err(Error::msg(
                "the integrity marker needs one capture group around the stored value",
            ));
        }

        Ok(regex)
    }

    fn digest(&self, body: &str) -> u64 {
        match (self.checksum, self.skip_whitespace) {
            (Checksum::Murmur3, true) => {
                u64::from(murmur3_skipping_whitespace(body.as_bytes(), self.seed))
            }
            (other, true) => {
                let filtered: Vec<u8> = body
                    .bytes()
                    .filter(|byte| !byte.is_ascii_whitespace())
                    .collect();
                other.compute(&filtered, self.seed)
            }
            (other, false) => other.compute(body.as_bytes(), self.seed),
        }
    }

    fn covered(&self, source: &str, whole: (usize, usize), value: (usize, usize)) -> String {
        match self.scope {
            Scope::ExcludingMarker => {
                let mut out = String::with_capacity(source.len());
                out.push_str(&source[..whole.0]);
                out.push_str(&source[whole.1..]);
                out
            }
            Scope::AfterMarker => source[whole.1..].to_string(),
            Scope::MarkerZeroed => {
                let mut out = String::with_capacity(source.len());
                out.push_str(&source[..value.0]);
                out.push('0');
                out.push_str(&source[value.1..]);
                out
            }
        }
    }

    pub fn verify(&self, source: &str) -> Result<Report> {
        let regex = self.compiled()?;

        let captures = regex
            .captures(source)
            .ok_or_else(|| Error::msg("the integrity marker was not found in the source"))?;

        let whole = captures.get(0).expect("group zero always exists");
        let stored_text = captures
            .get(1)
            .ok_or_else(|| Error::msg("the integrity marker matched but captured nothing"))?;

        let stored: u64 = stored_text
            .as_str()
            .trim()
            .parse()
            .map_err(|_| {
                Error::msg(format!(
                    "the stored integrity value {:?} is not a number",
                    stored_text.as_str()
                ))
            })?;

        let body = self.covered(
            source,
            (whole.start(), whole.end()),
            (stored_text.start(), stored_text.end()),
        );

        let computed = self.digest(&body);

        Ok(Report {
            stored,
            computed,
            covered_bytes: body.len(),
        })
    }

    pub fn resign(&self, source: &str) -> Result<(String, Report)> {
        let regex = self.compiled()?;

        let captures = regex
            .captures(source)
            .ok_or_else(|| Error::msg("the integrity marker was not found in the source"))?;

        let whole = captures.get(0).expect("group zero always exists");
        let stored_text = captures
            .get(1)
            .ok_or_else(|| Error::msg("the integrity marker matched but captured nothing"))?;

        let stored: u64 = stored_text.as_str().trim().parse().unwrap_or_default();

        let body = self.covered(
            source,
            (whole.start(), whole.end()),
            (stored_text.start(), stored_text.end()),
        );

        let computed = self.digest(&body);

        let mut out = String::with_capacity(source.len() + 8);
        out.push_str(&source[..stored_text.start()]);
        out.push_str(&computed.to_string());
        out.push_str(&source[stored_text.end()..]);

        Ok((
            out,
            Report {
                stored,
                computed,
                covered_bytes: body.len(),
            },
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub stored: u64,
    pub computed: u64,
    pub covered_bytes: usize,
}

impl Report {
    pub fn holds(&self) -> bool {
        self.stored == self.computed
    }

    pub fn describe(&self) -> String {
        if self.holds() {
            format!(
                "integrity holds, {} covers {} bytes",
                self.computed, self.covered_bytes
            )
        } else {
            format!(
                "integrity broken, the script stores {} but its {} bytes hash to {}",
                self.stored, self.covered_bytes, self.computed
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARKER: &str = r"/\* guard:(\d+) \*/";

    fn guarded(scope: Scope) -> Guard {
        Guard::new(MARKER).seeded(35_549).over(scope)
    }

    fn sign(source: &str, scope: Scope) -> String {
        guarded(scope).resign(source).unwrap().0
    }

    #[test]
    fn a_freshly_signed_script_verifies() {
        let source = "/* guard:0 */ function run() { return 1; }";

        for scope in [Scope::ExcludingMarker, Scope::AfterMarker, Scope::MarkerZeroed] {
            let signed = sign(source, scope);
            let report = guarded(scope).verify(&signed).unwrap();
            assert!(report.holds(), "{scope:?}: {}", report.describe());
        }
    }

    #[test]
    fn editing_the_body_breaks_the_guard() {
        let signed = sign("/* guard:0 */ function run() { return 1; }", Scope::AfterMarker);
        let edited = signed.replace("return 1", "return 2");

        let report = guarded(Scope::AfterMarker).verify(&edited).unwrap();
        assert!(!report.holds());
        assert!(report.describe().contains("integrity broken"));
    }

    #[test]
    fn resigning_after_an_edit_makes_it_verify_again() {
        let signed = sign("/* guard:0 */ function run() { return 1; }", Scope::AfterMarker);
        let edited = signed.replace("return 1", "return 2");

        let (resigned, report) = guarded(Scope::AfterMarker).resign(&edited).unwrap();
        assert!(!report.holds(), "the report describes the state before resigning");
        assert!(guarded(Scope::AfterMarker).verify(&resigned).unwrap().holds());
    }

    #[test]
    fn whitespace_only_edits_are_tolerated_when_asked() {
        let dense = "/* guard:0 */ function run(){return 1}";
        let reformat = |text: &str| {
            text.replace("function run(){return 1}", "function run() {\n  return 1\n}")
        };

        let loose = Guard::new(MARKER)
            .seeded(35_549)
            .over(Scope::AfterMarker)
            .skipping_whitespace();

        let signed = loose.resign(dense).unwrap().0;
        assert!(loose.verify(&reformat(&signed)).unwrap().holds());

        let strict = guarded(Scope::AfterMarker);
        let strict_signed = strict.resign(dense).unwrap().0;
        assert!(!strict.verify(&reformat(&strict_signed)).unwrap().holds());
    }

    #[test]
    fn a_missing_marker_is_reported() {
        let error = guarded(Scope::AfterMarker)
            .verify("function run() {}")
            .unwrap_err()
            .to_string();
        assert!(error.contains("was not found"), "{error}");
    }

    #[test]
    fn a_marker_without_a_capture_group_is_rejected() {
        let error = Guard::new(r"guard")
            .verify("guard")
            .unwrap_err()
            .to_string();
        assert!(error.contains("one capture group"), "{error}");
    }

    #[test]
    fn a_marker_that_does_not_compile_is_reported() {
        let error = Guard::new("([unclosed").verify("x").unwrap_err().to_string();
        assert!(error.contains("does not compile"), "{error}");
    }

    #[test]
    fn a_non_numeric_stored_value_is_reported() {
        let error = Guard::new(r"guard:(\w+);")
            .verify("guard:abc;")
            .unwrap_err()
            .to_string();
        assert!(error.contains("is not a number"), "{error}");
    }

    #[test]
    fn the_three_scopes_cover_different_bytes() {
        let source = "var head = 1; /* guard:0 */ function run() { return 1; }";

        let excluding = guarded(Scope::ExcludingMarker).resign(source).unwrap().1;
        let after = guarded(Scope::AfterMarker).resign(source).unwrap().1;
        let zeroed = guarded(Scope::MarkerZeroed).resign(source).unwrap().1;

        assert!(
            after.covered_bytes < excluding.covered_bytes,
            "after {} should skip the head that excluding {} keeps",
            after.covered_bytes,
            excluding.covered_bytes
        );
        assert!(excluding.covered_bytes < zeroed.covered_bytes);
        assert_ne!(excluding.computed, after.computed);
        assert_ne!(excluding.computed, zeroed.computed);
    }

    #[test]
    fn a_guard_round_trips_through_json() {
        let guard = guarded(Scope::MarkerZeroed).skipping_whitespace();
        let text = serde_json::to_string(&guard).unwrap();
        assert_eq!(serde_json::from_str::<Guard>(&text).unwrap(), guard);
    }
}
