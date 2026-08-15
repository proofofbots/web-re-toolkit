use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub name: String,
    pub group: String,
    pub source: String,
    pub note: String,
}

impl Marker {
    fn new(name: &str, group: &str, note: &str, source: &str) -> Self {
        Self {
            name: name.to_string(),
            group: group.to_string(),
            source: source.to_string(),
            note: note.to_string(),
        }
    }
}

pub fn automation_markers() -> Vec<Marker> {
    vec![
        Marker::new(
            "webdriver-true",
            "navigator",
            "navigator.webdriver reads true, the flag a plain automated browser sets",
            "Object.defineProperty(Navigator.prototype, 'webdriver', { get: function () { return true; }, configurable: true });",
        ),
        Marker::new(
            "cdc-globals",
            "globals",
            "the chromedriver document variables",
            "window.cdc_adoQpoasnfa76pfcZLmcfl_Array = window.Array; window.cdc_adoQpoasnfa76pfcZLmcfl_Promise = window.Promise; window.cdc_adoQpoasnfa76pfcZLmcfl_Symbol = window.Symbol;",
        ),
        Marker::new(
            "selenium-globals",
            "globals",
            "the selenium injected globals",
            "window._selenium = {}; window.__selenium_evaluate = function () {}; window.__webdriver_evaluate = function () {}; window.__driver_evaluate = function () {};",
        ),
        Marker::new(
            "phantom-globals",
            "globals",
            "the phantomjs callback pair",
            "window._phantom = {}; window.callPhantom = function () {};",
        ),
        Marker::new(
            "headless-user-agent",
            "navigator",
            "HeadlessChrome left in the user agent string",
            "Object.defineProperty(Navigator.prototype, 'userAgent', { get: function () { return navigator.appVersion.indexOf('Headless') >= 0 ? navigator.appVersion : 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/140.0.0.0 Safari/537.36'; }, configurable: true });",
        ),
        Marker::new(
            "no-plugins",
            "navigator",
            "an empty plugin array, which a real desktop Chrome does not have",
            "Object.defineProperty(Navigator.prototype, 'plugins', { get: function () { return []; }, configurable: true });",
        ),
        Marker::new(
            "no-languages",
            "navigator",
            "an empty languages list",
            "Object.defineProperty(Navigator.prototype, 'languages', { get: function () { return []; }, configurable: true });",
        ),
        Marker::new(
            "permissions-mismatch",
            "permissions",
            "notification permission denied while Notification.permission says default",
            "navigator.permissions.query = function () { return Promise.resolve({ state: 'denied' }); };",
        ),
        Marker::new(
            "broken-tostring",
            "functions",
            "a patched function whose toString gives it away",
            "var real = Function.prototype.toString; Function.prototype.toString = function () { return 'function () { [native code] }'; };",
        ),
        Marker::new(
            "zero-chrome-runtime",
            "globals",
            "window.chrome present but without runtime, as in some automated builds",
            "window.chrome = { runtime: undefined };",
        ),
        Marker::new(
            "notification-permission",
            "permissions",
            "Notification.permission forced to denied",
            "Object.defineProperty(Notification, 'permission', { get: function () { return 'denied'; }, configurable: true });",
        ),
        Marker::new(
            "zero-outer-dimensions",
            "window",
            "outerWidth and outerHeight of zero, the classic headless tell",
            "Object.defineProperty(window, 'outerWidth', { get: function () { return 0; }, configurable: true }); Object.defineProperty(window, 'outerHeight', { get: function () { return 0; }, configurable: true });",
        ),
        Marker::new(
            "webgl-swiftshader",
            "graphics",
            "a software renderer string in the WebGL debug extension",
            "var getParameter = WebGLRenderingContext.prototype.getParameter; WebGLRenderingContext.prototype.getParameter = function (name) { if (name === 37446) return 'Google SwiftShader'; if (name === 37445) return 'Google Inc.'; return getParameter.call(this, name); };",
        ),
        Marker::new(
            "no-media-devices",
            "hardware",
            "an empty media device list",
            "navigator.mediaDevices.enumerateDevices = function () { return Promise.resolve([]); };",
        ),
    ]
}

pub fn by_name(name: &str) -> Option<Marker> {
    automation_markers()
        .into_iter()
        .find(|marker| marker.name == name)
}

pub fn groups() -> Vec<String> {
    let mut out: Vec<String> = automation_markers()
        .into_iter()
        .map(|marker| marker.group)
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_marker_has_a_body() {
        for marker in automation_markers() {
            assert!(!marker.source.trim().is_empty(), "{} has no source", marker.name);
            assert!(!marker.note.trim().is_empty(), "{} has no note", marker.name);
        }
    }

    #[test]
    fn names_are_unique() {
        let markers = automation_markers();
        let mut names: Vec<&str> = markers.iter().map(|marker| marker.name.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len());
    }

    #[test]
    fn lookup_by_name_works() {
        assert!(by_name("webdriver-true").is_some());
        assert!(by_name("nothing-like-this").is_none());
        assert!(groups().contains(&"navigator".to_string()));
    }
}
