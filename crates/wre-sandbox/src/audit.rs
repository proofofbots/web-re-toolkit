use serde::{Deserialize, Serialize};

use crate::profile::Profile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Warn,
    Note,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Warn => "warn",
            Level::Note => "note",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub level: Level,
    pub what: String,
}

impl Finding {
    fn warn(what: impl Into<String>) -> Self {
        Self { level: Level::Warn, what: what.into() }
    }

    fn note(what: impl Into<String>) -> Self {
        Self { level: Level::Note, what: what.into() }
    }
}

fn text<'a>(profile: &'a Profile, brand: &str, name: &str) -> Option<&'a str> {
    profile.property(brand, name).and_then(|value| value.as_str())
}

fn number(profile: &Profile, brand: &str, name: &str) -> Option<f64> {
    profile.property(brand, name).and_then(|value| value.as_f64())
}

pub fn audit(profile: &Profile) -> Vec<Finding> {
    let mut findings = Vec::new();

    let agent = text(profile, "Navigator", "userAgent").unwrap_or_default();
    if agent.is_empty() {
        findings.push(Finding::warn("no navigator.userAgent"));
    }
    if agent.contains("HeadlessChrome") {
        findings.push(Finding::warn("the user agent says HeadlessChrome"));
    }

    if profile.property("Navigator", "webdriver") == Some(&serde_json::Value::Bool(true)) {
        findings.push(Finding::warn("navigator.webdriver is true, this browser was automated"));
    }

    let renderer = profile
        .webgl_parameters
        .get("37446")
        .and_then(|value| value.as_str())
        .unwrap_or_default();

    for marker in ["SwiftShader", "llvmpipe", "Mesa OffScreen", "Google SwiftShader"] {
        if renderer.contains(marker) {
            findings.push(Finding::warn(format!(
                "the WebGL renderer is {marker}, that is software rendering, not a real GPU"
            )));
        }
    }

    if renderer.is_empty() {
        findings.push(Finding::note("no unmasked WebGL renderer, parameter 37446 is missing"));
    }

    if profile.plugins.is_empty() && agent.contains("Chrome") && !agent.contains("Android") {
        findings.push(Finding::warn("a desktop Chrome with no plugins, real ones list the PDF viewers"));
    }

    let platform = text(profile, "Navigator", "platform").unwrap_or_default();
    let pairs = [
        ("MacIntel", "Macintosh"),
        ("Win32", "Windows"),
        ("Linux x86_64", "Linux"),
        ("iPhone", "iPhone"),
    ];
    for (value, expected) in pairs {
        if platform == value && !agent.contains(expected) {
            findings.push(Finding::warn(format!(
                "navigator.platform is {value} but the user agent does not mention {expected}"
            )));
        }
    }

    let inner_height = number(profile, "Window", "innerHeight");
    let outer_height = number(profile, "Window", "outerHeight");
    let screen_height = number(profile, "Screen", "height");

    if let (Some(inner), Some(outer)) = (inner_height, outer_height)
        && inner > outer
    {
        findings.push(Finding::warn(format!(
            "innerHeight {inner} is taller than outerHeight {outer}"
        )));
    }

    if let (Some(outer), Some(screen)) = (outer_height, screen_height)
        && outer > screen
    {
        findings.push(Finding::warn(format!(
            "outerHeight {outer} does not fit on a {screen} tall screen"
        )));
    }

    if let (Some(avail), Some(screen)) =
        (number(profile, "Screen", "availHeight"), screen_height)
        && avail > screen
    {
        findings.push(Finding::warn(format!(
            "availHeight {avail} is taller than the screen at {screen}"
        )));
    }

    if number(profile, "Navigator", "hardwareConcurrency").unwrap_or(0.0) < 1.0 {
        findings.push(Finding::note("navigator.hardwareConcurrency is missing or zero"));
    }

    let touch = number(profile, "Navigator", "maxTouchPoints").unwrap_or(0.0);
    let mobile = agent.contains("Mobile") || agent.contains("Android") || agent.contains("iPhone");
    if mobile && touch < 1.0 {
        findings.push(Finding::warn("a mobile user agent with maxTouchPoints 0"));
    }

    if let Some(data) = &profile.user_agent_data {
        let claimed = data
            .brands
            .iter()
            .find(|brand| brand.brand.contains("Chrome") || brand.brand.contains("Chromium"))
            .map(|brand| brand.version.clone())
            .unwrap_or_default();

        let running = agent
            .split("Chrome/")
            .nth(1)
            .and_then(|rest| rest.split('.').next())
            .unwrap_or_default()
            .to_string();

        if !claimed.is_empty() && !running.is_empty() && claimed != running {
            findings.push(Finding::warn(format!(
                "userAgentData says Chrome {claimed} but the user agent says Chrome {running}"
            )));
        }
    }

    if let Some(intl) = &profile.intl
        && intl.time_zone.is_empty()
    {
        findings.push(Finding::note("no timezone was captured"));
    }

    for (what, empty) in [
        ("webgl_extensions", profile.webgl_extensions.is_empty()),
        ("webgl_parameters", profile.webgl_parameters.is_empty()),
        ("media_support", profile.media_support.is_empty()),
        ("media_queries", profile.media_queries.is_empty()),
        ("permissions", profile.permissions.is_empty()),
        ("font_widths", profile.font_widths.is_empty()),
        ("window_order", profile.window_order.is_empty()),
        ("canvas", profile.canvas.is_empty()),
        ("voices", profile.voices.is_empty()),
        ("media_devices", profile.media_devices.is_empty()),
        ("mime_types", profile.mime_types.is_empty()),
    ] {
        if empty {
            findings.push(Finding::note(format!("{what} is empty, the sandbox will record misses")));
        }
    }

    findings
}

pub fn warnings(findings: &[Finding]) -> usize {
    findings.iter().filter(|finding| finding.level == Level::Warn).count()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_builtin_profile_raises_no_warnings() {
        let findings = audit(&Profile::desktop_chrome());
        assert_eq!(warnings(&findings), 0, "{findings:?}");
    }

    #[test]
    fn an_automated_browser_is_called_out() {
        let mut profile = Profile::desktop_chrome();
        profile.set("Navigator", "webdriver", json!(true)).unwrap();
        profile
            .webgl_parameters
            .insert("37446".to_string(), json!("Google SwiftShader"));
        profile.plugins.clear();

        let findings = audit(&profile);
        assert!(warnings(&findings) >= 3, "{findings:?}");
        assert!(findings.iter().any(|finding| finding.what.contains("webdriver")));
    }

    #[test]
    fn impossible_geometry_is_called_out() {
        let mut profile = Profile::desktop_chrome();
        profile.set("Window", "innerHeight", json!(4000)).unwrap();

        let findings = audit(&profile);
        assert!(findings.iter().any(|finding| finding.what.contains("taller than outerHeight")));
    }
}
