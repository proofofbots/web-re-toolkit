use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    #[default]
    Presence,
    Concealment,
}

impl Kind {
    pub fn describe(self) -> &'static str {
        match self {
            Kind::Presence => "something an automated browser leaves behind",
            Kind::Concealment => "an attempt to hide a tell, which is a tell of its own",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub name: String,
    pub group: String,
    pub source: String,
    pub note: String,
    #[serde(default)]
    pub kind: Kind,
}

impl Marker {
    fn new(name: &str, group: &str, note: &str, source: &str) -> Self {
        Self {
            name: name.to_string(),
            group: group.to_string(),
            source: source.to_string(),
            note: note.to_string(),
            kind: Kind::Presence,
        }
    }

    fn concealment(name: &str, group: &str, note: &str, source: &str) -> Self {
        Self { kind: Kind::Concealment, ..Self::new(name, group, note, source) }
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
        Marker::new(
            "webdriver-function-globals",
            "globals",
            "the __webdriver_* and __driver_* function pairs several drivers install",
            "window.__webdriverFuncgeb = function () {}; window.__webdriver__chr = true; window.__webdriver_script_fn = function () {}; window.__webdriver_script_func = function () {}; window.__webdriver_unwrapped = function () {}; window.__driver_unwrapped = function () {};",
        ),
        Marker::new(
            "fxdriver-globals",
            "globals",
            "the firefox driver evaluate pair",
            "window.__fxdriver_evaluate = function () {}; window.__fxdriver_unwrapped = function () {};",
        ),
        Marker::new(
            "selenium-recorder-globals",
            "globals",
            "the selenium ide recorder and its async executor",
            "window._Selenium_IDE_Recorder = {}; window.__$webdriverAsyncExecutor = {}; window.__selenium_unwrapped = function () {}; window.callSelenium = function () {}; window.calledSelenium = true;",
        ),
        Marker::new(
            "playwright-globals",
            "globals",
            "the playwright recorder, clock and init script hooks",
            "window.__pw_recorderRecordAction = function () {}; window.__pw_recorderState = {}; window.__pw_devtools__ = {}; window.__pwClock = {}; window.__pwInitScripts = {};",
        ),
        Marker::new(
            "puppeteer-stealth-globals",
            "globals",
            "the helper names puppeteer-extra stealth leaves in scope",
            "window.runHeadlessFixes = function () {}; window.overrideStatic = function () {}; window.addContentWindowProxy = function () {};",
        ),
        Marker::new(
            "nightmare-globals",
            "globals",
            "the nightmare bridge object",
            "window.__nightmare = {}; window.__nightmare_ipc = {};",
        ),
        Marker::new(
            "phantomas-globals",
            "globals",
            "the phantomas instrumentation pair",
            "window.__phantomas = {}; window.calledPhantom = true;",
        ),
        Marker::new(
            "watir-globals",
            "globals",
            "the watir and watin dialog recorders",
            "window.__lastWatirAlert = ''; window.__lastWatirConfirm = ''; window.__lastWatirPrompt = ''; window.watinExpressionError = ''; window.watinExpressionResult = '';",
        ),
        Marker::new(
            "spynner-globals",
            "globals",
            "the spynner load flag",
            "window.spynner_additional_js_loaded = true;",
        ),
        Marker::new(
            "dom-automation-controller",
            "globals",
            "the chrome automation controller binding",
            "window.domAutomationController = { send: function () {} };",
        ),
        Marker::new(
            "cefsharp-global",
            "globals",
            "the embedded chromium bridge object",
            "window.CefSharp = { PostMessage: function () {} };",
        ),
        Marker::new(
            "awesomium-global",
            "globals",
            "the awesomium embedded browser bridge",
            "window.awesomium = {};",
        ),
        Marker::new(
            "cdp-binding-names",
            "globals",
            "an exposed binding function, which counts as an extra own property of window",
            "window.__wreExposedBinding = function () {}; Object.defineProperty(window.__wreExposedBinding, 'name', { value: 'cdpBinding' });",
        ),
        Marker::new(
            "hardware-concurrency-one",
            "hardware",
            "a single logical core, rare on the desktops these builds claim to be",
            "Object.defineProperty(Navigator.prototype, 'hardwareConcurrency', { get: function () { return 1; }, configurable: true });",
        ),
        Marker::new(
            "device-memory-missing",
            "hardware",
            "deviceMemory absent on a build whose user agent says chrome",
            "delete Navigator.prototype.deviceMemory;",
        ),
        Marker::new(
            "no-battery",
            "hardware",
            "getBattery missing or rejecting",
            "Navigator.prototype.getBattery = function () { return Promise.reject(new Error('not supported')); };",
        ),
        Marker::new(
            "no-gamepads",
            "hardware",
            "an empty gamepad list where a real build returns four null slots",
            "Navigator.prototype.getGamepads = function () { return []; };",
        ),
        Marker::new(
            "shared-array-buffer-missing",
            "globals",
            "SharedArrayBuffer removed, which also implies the isolation headers are absent",
            "delete window.SharedArrayBuffer;",
        ),
        Marker::new(
            "no-speech-voices",
            "media",
            "an empty speech synthesis voice list",
            "window.speechSynthesis.getVoices = function () { return []; };",
        ),
        Marker::new(
            "canplaytype-empty",
            "media",
            "canPlayType answering empty for every codec",
            "HTMLMediaElement.prototype.canPlayType = function () { return ''; };",
        ),
        Marker::new(
            "no-webrtc",
            "network",
            "RTCPeerConnection removed, so the ice candidate probe finds nothing",
            "delete window.RTCPeerConnection; delete window.webkitRTCPeerConnection;",
        ),
        Marker::new(
            "zero-inner-dimensions",
            "window",
            "an inner viewport of zero, which no rendered page has",
            "Object.defineProperty(window, 'innerWidth', { get: function () { return 0; }, configurable: true }); Object.defineProperty(window, 'innerHeight', { get: function () { return 0; }, configurable: true });",
        ),
        Marker::new(
            "screen-equals-viewport",
            "window",
            "screen and viewport exactly equal, so the browser reports no chrome at all",
            "Object.defineProperty(Screen.prototype, 'availWidth', { get: function () { return window.innerWidth; }, configurable: true }); Object.defineProperty(Screen.prototype, 'availHeight', { get: function () { return window.innerHeight; }, configurable: true });",
        ),
        Marker::new(
            "no-screen-orientation",
            "window",
            "screen.orientation missing",
            "delete Screen.prototype.orientation;",
        ),
        Marker::new(
            "colour-depth-mismatch",
            "window",
            "a colour depth no real display reports",
            "Object.defineProperty(Screen.prototype, 'colorDepth', { get: function () { return 8; }, configurable: true });",
        ),
        Marker::new(
            "empty-mimetypes",
            "navigator",
            "an empty mimeTypes table alongside a populated plugin list",
            "Object.defineProperty(Navigator.prototype, 'mimeTypes', { get: function () { return []; }, configurable: true });",
        ),
        Marker::new(
            "platform-disagrees-with-agent",
            "navigator",
            "navigator.platform saying linux while the user agent claims macintosh",
            "Object.defineProperty(Navigator.prototype, 'platform', { get: function () { return 'Linux x86_64'; }, configurable: true });",
        ),
        Marker::new(
            "no-user-agent-data",
            "navigator",
            "userAgentData missing on a build whose user agent claims a recent chrome",
            "delete Navigator.prototype.userAgentData;",
        ),
        Marker::new(
            "timezone-disagrees-with-offset",
            "locale",
            "Intl reporting one zone while getTimezoneOffset reports another",
            "Date.prototype.getTimezoneOffset = function () { return 0; };",
        ),
        Marker::new(
            "no-indexeddb",
            "storage",
            "indexedDB removed",
            "delete window.indexedDB;",
        ),
        Marker::new(
            "storage-throws",
            "storage",
            "localStorage present but throwing on write, as in a blocked context",
            "Object.defineProperty(window, 'localStorage', { get: function () { throw new DOMException('denied', 'SecurityError'); }, configurable: true });",
        ),
        Marker::new(
            "no-storage-estimate",
            "storage",
            "the quota estimate missing",
            "delete StorageManager.prototype.estimate;",
        ),
        Marker::new(
            "canvas-blank",
            "graphics",
            "a canvas that renders nothing, so every fingerprint hashes the same",
            "HTMLCanvasElement.prototype.toDataURL = function () { return 'data:image/png;base64,iVBORw0KGgo='; };",
        ),
        Marker::new(
            "canvas-noise",
            "graphics",
            "per read noise in the canvas, which makes the same page hash differently twice",
            "var getImageData = CanvasRenderingContext2D.prototype.getImageData; CanvasRenderingContext2D.prototype.getImageData = function () { var data = getImageData.apply(this, arguments); for (var i = 0; i < data.data.length; i += 997) { data.data[i] ^= 1; } return data; };",
        ),
        Marker::new(
            "no-webgl2",
            "graphics",
            "webgl2 unavailable while webgl1 works",
            "var getContext = HTMLCanvasElement.prototype.getContext; HTMLCanvasElement.prototype.getContext = function (kind) { if (kind === 'webgl2') return null; return getContext.apply(this, arguments); };",
        ),
        Marker::new(
            "no-webgl-debug-info",
            "graphics",
            "the unmasked vendor extension missing, which real desktop chrome exposes",
            "var getExtension = WebGLRenderingContext.prototype.getExtension; WebGLRenderingContext.prototype.getExtension = function (name) { if (name === 'WEBGL_debug_renderer_info') return null; return getExtension.apply(this, arguments); };",
        ),
        Marker::new(
            "audio-context-silent",
            "audio",
            "an offline audio render that returns silence",
            "var getChannelData = AudioBuffer.prototype.getChannelData; AudioBuffer.prototype.getChannelData = function () { var data = getChannelData.apply(this, arguments); data.fill(0); return data; };",
        ),
        Marker::new(
            "no-fonts",
            "fonts",
            "every font measuring the same width, so the font probe finds one family",
            "Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { get: function () { return 100; }, configurable: true });",
        ),
        Marker::new(
            "error-stack-empty",
            "errors",
            "an empty stack on every error, which hides the frame count a real engine gives",
            "Object.defineProperty(Error.prototype, 'stack', { get: function () { return ''; }, configurable: true });",
        ),
        Marker::new(
            "prepare-stack-trace-set",
            "errors",
            "Error.prepareStackTrace replaced, which the v8 stack format probe notices",
            "Error.prepareStackTrace = function () { return ''; };",
        ),
        Marker::new(
            "no-devtools-detect",
            "errors",
            "a console.debug that never formats its argument, so the devtools probe reads closed",
            "console.debug = function () {};",
        ),
        Marker::concealment(
            "webdriver-hidden",
            "concealment",
            "navigator.webdriver forced false through a getter, which leaves an accessor where a data property belongs",
            "Object.defineProperty(Navigator.prototype, 'webdriver', { get: function () { return false; }, configurable: true });",
        ),
        Marker::concealment(
            "webdriver-deleted",
            "concealment",
            "the webdriver property deleted from the prototype rather than set false",
            "delete Navigator.prototype.webdriver;",
        ),
        Marker::concealment(
            "tostring-proxied",
            "concealment",
            "Function.prototype.toString routed through a proxy so patched natives read clean",
            "Function.prototype.toString = new Proxy(Function.prototype.toString, { apply: function (target, self, args) { return Reflect.apply(target, self, args); } });",
        ),
        Marker::concealment(
            "user-agent-proxied",
            "concealment",
            "the user agent served through a proxy, which changes the property descriptor kind",
            "var navigatorProxy = new Proxy(navigator, { get: function (target, key) { return Reflect.get(target, key); } }); Object.defineProperty(window, 'navigator', { get: function () { return navigatorProxy; }, configurable: true });",
        ),
        Marker::concealment(
            "plugins-faked",
            "concealment",
            "a hand built plugin array whose entries are plain objects rather than Plugin instances",
            "Object.defineProperty(Navigator.prototype, 'plugins', { get: function () { return [{ name: 'Chrome PDF Plugin' }, { name: 'Chrome PDF Viewer' }]; }, configurable: true });",
        ),
        Marker::concealment(
            "chrome-runtime-faked",
            "concealment",
            "a hand built window.chrome whose members are plain functions",
            "window.chrome = { runtime: {}, loadTimes: function () {}, csi: function () {}, app: { isInstalled: false } };",
        ),
        Marker::concealment(
            "permissions-query-patched",
            "concealment",
            "permissions.query replaced so the notification answer agrees with Notification.permission",
            "var query = navigator.permissions.query; navigator.permissions.query = function (spec) { return spec && spec.name === 'notifications' ? Promise.resolve({ state: Notification.permission }) : query.apply(navigator.permissions, arguments); };",
        ),
        Marker::concealment(
            "getparameter-patched",
            "concealment",
            "WebGL getParameter replaced to report a discrete gpu",
            "var getParameter = WebGLRenderingContext.prototype.getParameter; WebGLRenderingContext.prototype.getParameter = function (name) { if (name === 37445) return 'Intel Inc.'; if (name === 37446) return 'Intel Iris OpenGL Engine'; return getParameter.apply(this, arguments); };",
        ),
        Marker::concealment(
            "stack-scrubbed",
            "concealment",
            "proxy frames stripped out of error stacks, which shortens the stack a real throw produces",
            "var descriptor = Object.getOwnPropertyDescriptor(Error.prototype, 'stack'); Object.defineProperty(Error.prototype, 'stack', { get: function () { var text = descriptor && descriptor.get ? descriptor.get.call(this) : ''; return String(text).split('\\n').filter(function (line) { return line.indexOf('Proxy') < 0; }).join('\\n'); }, configurable: true });",
        ),
    ]
}

pub fn by_name(name: &str) -> Option<Marker> {
    automation_markers()
        .into_iter()
        .find(|marker| marker.name == name)
}

pub fn of_kind(kind: Kind) -> Vec<Marker> {
    automation_markers()
        .into_iter()
        .filter(|marker| marker.kind == kind)
        .collect()
}

pub fn in_group(group: &str) -> Vec<Marker> {
    automation_markers()
        .into_iter()
        .filter(|marker| marker.group == group)
        .collect()
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
