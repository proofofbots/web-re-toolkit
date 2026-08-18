use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use wre_core::error::{Error, Result};

static SCRIPT_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<script\b([^>]*)>"#).expect("script pattern"));

static INLINE_SCRIPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<script\b([^>]*)>(.*?)</script\s*>"#).expect("inline script pattern")
});

static FIELD_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<(input|textarea|select|button)\b([^>]*)>"#).expect("field pattern"));

static FORM_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<form\b([^>]*)>"#).expect("form pattern"));

static TITLE_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<title[^>]*>(.*?)</title>"#).expect("title pattern"));

static ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)([a-zA-Z_:][-a-zA-Z0-9_:.]*)\s*(?:=\s*("[^"]*"|'[^']*'|[^\s"'>]+))?"#)
        .expect("attribute pattern")
});

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Field {
    pub tag: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    #[serde(default)]
    pub labels: usize,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default = "no_form")]
    pub form: i64,
}

fn yes() -> bool {
    true
}

fn no_form() -> i64 {
    -1
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Form {
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub url: String,
    #[serde(default)]
    pub html: String,
    #[serde(default)]
    pub referrer: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub scripts: Vec<String>,
    pub current_script: String,
    #[serde(default)]
    pub fields: Vec<Field>,
    #[serde(default)]
    pub forms: Vec<Form>,
    #[serde(default)]
    pub epoch_ms: f64,
    #[serde(default)]
    pub friction_ms: f64,
    pub geometry: Option<Value>,
    #[serde(default)]
    pub html_limit: usize,
    #[serde(default)]
    field_offsets: Vec<usize>,
    #[serde(default)]
    script_offsets: Vec<(String, usize)>,
    #[serde(default)]
    inline_scripts: Vec<(usize, String)>,
}

impl Page {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            html: String::new(),
            referrer: String::new(),
            title: String::new(),
            scripts: Vec::new(),
            current_script: String::new(),
            fields: Vec::new(),
            forms: Vec::new(),
            epoch_ms: 0.0,
            friction_ms: 0.12,
            geometry: None,
            html_limit: 300_000,
            field_offsets: Vec::new(),
            script_offsets: Vec::new(),
            inline_scripts: Vec::new(),
        }
    }

    pub fn inline_scripts(&self) -> (Vec<String>, Vec<String>) {
        let boundary = self
            .script_offsets
            .iter()
            .find(|(src, _)| src == &self.current_script)
            .map(|(_, at)| *at)
            .unwrap_or(usize::MAX);

        let mut before = Vec::new();
        let mut after = Vec::new();

        for (at, source) in &self.inline_scripts {
            if *at < boundary {
                before.push(source.clone());
            } else {
                after.push(source.clone());
            }
        }

        (before, after)
    }

    pub fn fields_at_current_script(&self) -> usize {
        if self.current_script.is_empty() {
            return self.fields.len();
        }

        let Some((_, at)) = self
            .script_offsets
            .iter()
            .find(|(src, _)| src == &self.current_script)
        else {
            return self.fields.len();
        };

        self.field_offsets.iter().filter(|offset| *offset < at).count()
    }

    pub fn read(url: impl Into<String>, html: &str) -> Self {
        let mut page = Self::new(url);
        page.load(html);
        page
    }

    pub fn load(&mut self, html: &str) {
        let base = self.url.clone();

        self.title = TITLE_TAG
            .captures(html)
            .and_then(|found| found.get(1))
            .map(|found| found.as_str().trim().to_string())
            .unwrap_or_default();

        self.script_offsets = SCRIPT_TAG
            .captures_iter(html)
            .map(|found| {
                let at = found.get(0).map_or(0, |part| part.start());
                let attributes = attributes(found.get(1).map_or("", |part| part.as_str()));
                let src = match attributes.get("src") {
                    Some(src) => absolute(&base, src),
                    None => "[inline]".to_string(),
                };

                (src, at)
            })
            .collect();

        self.scripts = self.script_offsets.iter().map(|(src, _)| src.clone()).collect();

        self.inline_scripts = INLINE_SCRIPT
            .captures_iter(html)
            .filter(|found| !attributes(found.get(1).map_or("", |part| part.as_str())).contains_key("src"))
            .map(|found| {
                let at = found.get(0).map_or(0, |part| part.start());
                (at, found.get(2).map_or("", |part| part.as_str()).to_string())
            })
            .filter(|(_, source)| !source.trim().is_empty())
            .collect();

        self.forms = FORM_TAG
            .captures_iter(html)
            .map(|found| Form { attributes: attributes(found.get(1).map_or("", |part| part.as_str())) })
            .collect();

        self.field_offsets = FIELD_TAG
            .captures_iter(html)
            .map(|found| found.get(0).map_or(0, |part| part.start()))
            .collect();

        self.fields = FIELD_TAG
            .captures_iter(html)
            .map(|found| {
                let tag = found.get(1).map_or("input", |part| part.as_str()).to_lowercase();
                let attributes = attributes(found.get(2).map_or("", |part| part.as_str()));

                let hidden = attributes.get("type").map(|kind| kind == "hidden").unwrap_or(false)
                    || attributes.contains_key("hidden");

                Field {
                    tag,
                    labels: 0,
                    visible: !hidden,
                    form: if self.forms.is_empty() { -1 } else { 0 },
                    attributes,
                }
            })
            .collect();

        self.html = if html.len() > self.html_limit && self.html_limit > 0 {
            html[..self.html_limit].to_string()
        } else {
            html.to_string()
        };
    }

    pub fn with_referrer(mut self, referrer: impl Into<String>) -> Self {
        self.referrer = referrer.into();
        self
    }

    pub fn with_epoch(mut self, epoch_ms: f64) -> Self {
        self.epoch_ms = epoch_ms;
        self
    }

    pub fn with_friction(mut self, friction_ms: f64) -> Self {
        self.friction_ms = friction_ms;
        self
    }

    pub fn with_geometry(mut self, geometry: Value) -> Self {
        self.geometry = Some(geometry);
        self
    }

    pub fn running(mut self, url: impl Into<String>) -> Self {
        let url = url.into();
        self.current_script = url.clone();

        if !self.scripts.contains(&url) {
            self.scripts.push(url);
        }

        self
    }

    pub fn with_script(mut self, url: impl Into<String>) -> Self {
        let url = url.into();
        if !self.scripts.contains(&url) {
            self.scripts.push(url);
        }
        self
    }

    pub fn origin(&self) -> Result<String> {
        let parsed = self.parsed()?;
        Ok(format!(
            "{}://{}",
            parsed.scheme(),
            parsed.host_str().unwrap_or_default()
        ))
    }

    fn parsed(&self) -> Result<Url> {
        Url::parse(&self.url).map_err(|error| Error::msg(format!("{}: {error}", self.url)))
    }

    pub fn location(&self) -> Result<Value> {
        let parsed = self.parsed()?;
        let host = match parsed.port() {
            Some(port) => format!("{}:{port}", parsed.host_str().unwrap_or_default()),
            None => parsed.host_str().unwrap_or_default().to_string(),
        };

        Ok(json!({
            "href": parsed.as_str(),
            "protocol": format!("{}:", parsed.scheme()),
            "host": host,
            "hostname": parsed.host_str().unwrap_or_default(),
            "port": parsed.port().map(|port| port.to_string()).unwrap_or_default(),
            "pathname": parsed.path(),
            "search": parsed.query().map(|query| format!("?{query}")).unwrap_or_default(),
            "hash": parsed.fragment().map(|hash| format!("#{hash}")).unwrap_or_default(),
            "origin": format!("{}://{host}", parsed.scheme()),
        }))
    }

    pub fn describe(&self) -> Result<Value> {
        Ok(json!({
            "location": self.location()?,
            "html": self.html,
            "referrer": self.referrer,
            "title": self.title,
            "scripts": self.scripts,
            "current_script": self.current_script,
            "inputs": self.fields,
            "inputs_parsed": self.fields_at_current_script(),
            "forms": self.forms,
            "epoch": self.epoch_ms,
            "friction": self.friction_ms,
            "geometry": self.geometry.clone().unwrap_or(Value::Null),
        }))
    }
}

fn attributes(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();

    for found in ATTRIBUTE.captures_iter(text) {
        let Some(name) = found.get(1) else {
            continue;
        };

        let value = found
            .get(2)
            .map(|part| part.as_str().trim_matches(['"', '\'']).to_string())
            .unwrap_or_default();

        out.insert(name.as_str().to_lowercase(), value);
    }

    out
}

fn absolute(base: &str, href: &str) -> String {
    match Url::parse(base).and_then(|parsed| parsed.join(href)) {
        Ok(joined) => joined.to_string(),
        Err(_) => href.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
<!doctype html>
<html><head><title>Sign in</title>
<script src="/akam/13/abcdef"></script>
<script>var inline = 1;</script>
</head>
<body>
<form method="post" action="/identity/user/login" id="login">
  <input type="hidden" name="__RequestVerificationToken" value="secret-token">
  <input type="email" name="Username" required>
  <input type="password" name="Password">
  <textarea name="notes"></textarea>
</form>
</body></html>
"#;

    #[test]
    fn the_page_reads_its_scripts_forms_and_fields() {
        let page = Page::read("https://login.example.com/identity/user/login", SAMPLE);

        assert_eq!(page.title, "Sign in");
        assert_eq!(page.scripts.len(), 2);
        assert_eq!(page.scripts[0], "https://login.example.com/akam/13/abcdef");
        assert_eq!(page.scripts[1], "[inline]");
        assert_eq!(page.forms.len(), 1);
        assert_eq!(page.forms[0].attributes.get("action").map(String::as_str), Some("/identity/user/login"));
        assert_eq!(page.fields.len(), 4);
    }

    #[test]
    fn a_hidden_field_is_marked_invisible_and_keeps_its_value() {
        let page = Page::read("https://login.example.com/", SAMPLE);
        let hidden = &page.fields[0];

        assert_eq!(hidden.attributes.get("name").map(String::as_str), Some("__RequestVerificationToken"));
        assert_eq!(hidden.attributes.get("value").map(String::as_str), Some("secret-token"));
        assert!(!hidden.visible);
        assert!(page.fields[1].visible);
    }

    #[test]
    fn only_the_fields_above_the_running_script_count_as_parsed() {
        let html = r#"<html><body>
<input name="early">
<script src="/sensor.js"></script>
<input name="late">
</body></html>"#;

        let page = Page::read("https://example.com/", html).running("https://example.com/sensor.js");

        assert_eq!(page.fields.len(), 2);
        assert_eq!(page.fields_at_current_script(), 1);
    }

    #[test]
    fn the_location_splits_the_way_a_browser_reports_it() {
        let page = Page::new("https://www.example.com:8443/deep/page?a=1#top");
        let location = page.location().unwrap();

        assert_eq!(location["host"], "www.example.com:8443");
        assert_eq!(location["hostname"], "www.example.com");
        assert_eq!(location["port"], "8443");
        assert_eq!(location["pathname"], "/deep/page");
        assert_eq!(location["search"], "?a=1");
        assert_eq!(location["hash"], "#top");
        assert_eq!(location["origin"], "https://www.example.com:8443");
    }
}
