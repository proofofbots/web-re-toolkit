use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use wre_core::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plugin {
    pub name: String,
    pub filename: String,
    pub description: String,
    #[serde(default)]
    pub mime_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MimeType {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub suffixes: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub plugin: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Brand {
    pub brand: String,
    pub version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UserAgentData {
    #[serde(default)]
    pub brands: Vec<Brand>,
    #[serde(default)]
    pub mobile: bool,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub high_entropy: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    #[serde(default)]
    pub downlink: f64,
    #[serde(default)]
    pub effective_type: String,
    #[serde(default)]
    pub rtt: f64,
    #[serde(default)]
    pub save_data: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Intl {
    #[serde(default)]
    pub locale: String,
    #[serde(default)]
    pub calendar: String,
    #[serde(default)]
    pub numbering_system: String,
    #[serde(default)]
    pub time_zone: String,
    #[serde(default)]
    pub hour_cycle: String,
    #[serde(default)]
    pub timezone_offset: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Audio {
    #[serde(default)]
    pub sample_rate: f64,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub base_latency: f64,
    #[serde(default)]
    pub output_latency: f64,
    #[serde(default)]
    pub max_channel_count: u32,
    #[serde(default)]
    pub channel_count: u32,
    #[serde(default)]
    pub rendered: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Battery {
    pub charging: bool,
    pub level: f64,
    #[serde(default)]
    pub charging_time: Option<f64>,
    #[serde(default)]
    pub discharging_time: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Voice {
    pub name: String,
    pub lang: String,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub local_service: bool,
    #[serde(default)]
    pub voice_uri: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub kind: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub group_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    pub js_heap_size_limit: f64,
    pub total_js_heap_size: f64,
    pub used_js_heap_size: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Orientation {
    pub angle: f64,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Interface {
    pub brand: String,
    pub constructor: String,
    pub instance: String,
    pub properties: BTreeMap<String, Value>,
}

impl Interface {
    pub fn new(brand: &str, constructor: &str, instance: &str) -> Self {
        Self {
            brand: brand.to_string(),
            constructor: constructor.to_string(),
            instance: instance.to_string(),
            properties: BTreeMap::new(),
        }
    }

    pub fn with(mut self, name: &str, value: Value) -> Self {
        self.properties.insert(name.to_string(), value);
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub interfaces: Vec<Interface>,
    #[serde(default)]
    pub plugins: Vec<Plugin>,
    #[serde(default)]
    pub mime_types: Vec<MimeType>,
    #[serde(default)]
    pub user_agent_data: Option<UserAgentData>,
    #[serde(default)]
    pub connection: Option<Connection>,
    #[serde(default)]
    pub intl: Option<Intl>,
    #[serde(default)]
    pub webgl_parameters: BTreeMap<String, Value>,
    #[serde(default)]
    pub webgl_extensions: Vec<String>,
    #[serde(default)]
    pub webgl2_parameters: BTreeMap<String, Value>,
    #[serde(default)]
    pub webgl2_extensions: Vec<String>,
    #[serde(default)]
    pub media_support: BTreeMap<String, String>,
    #[serde(default)]
    pub media_queries: BTreeMap<String, bool>,
    #[serde(default)]
    pub permissions: BTreeMap<String, String>,
    #[serde(default)]
    pub font_widths: BTreeMap<String, f64>,
    #[serde(default)]
    pub font_probe: String,
    #[serde(default)]
    pub layout: BTreeMap<String, Value>,
    #[serde(default)]
    pub canvas: BTreeMap<String, String>,
    #[serde(default)]
    pub audio: Option<Audio>,
    #[serde(default)]
    pub battery: Option<Battery>,
    #[serde(default)]
    pub voices: Vec<Voice>,
    #[serde(default)]
    pub media_devices: Vec<Device>,
    #[serde(default)]
    pub memory: Option<Memory>,
    #[serde(default)]
    pub orientation: Option<Orientation>,
    #[serde(default)]
    pub chrome: Option<Value>,
    #[serde(default)]
    pub document: BTreeMap<String, Value>,
    #[serde(default)]
    pub window_order: Vec<String>,
}

impl Profile {
    pub fn interface(&self, brand: &str) -> Option<&Interface> {
        self.interfaces.iter().find(|entry| entry.brand == brand)
    }

    pub fn property(&self, brand: &str, name: &str) -> Option<&Value> {
        self.interface(brand)?.properties.get(name)
    }

    pub fn set(&mut self, brand: &str, name: &str, value: Value) -> Result<()> {
        let entry = self
            .interfaces
            .iter_mut()
            .find(|entry| entry.brand == brand)
            .ok_or_else(|| Error::msg(format!("the profile has no {brand} interface")))?;

        entry.properties.insert(name.to_string(), value);
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        let mut seen: Vec<&str> = Vec::new();

        for entry in &self.interfaces {
            if entry.brand.is_empty() || entry.constructor.is_empty() {
                return Err(Error::msg("every interface needs a brand and a constructor"));
            }
            if seen.contains(&entry.brand.as_str()) {
                return Err(Error::msg(format!("two interfaces share the brand {}", entry.brand)));
            }
            seen.push(&entry.brand);
        }

        Ok(())
    }

    pub fn desktop_chrome() -> Self {
        let user_agent = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

        let navigator = Interface::new("Navigator", "Navigator", "navigator")
            .with("userAgent", json!(user_agent))
            .with("appVersion", json!(&user_agent[8..]))
            .with("appName", json!("Netscape"))
            .with("appCodeName", json!("Mozilla"))
            .with("product", json!("Gecko"))
            .with("productSub", json!("20030107"))
            .with("vendor", json!("Google Inc."))
            .with("vendorSub", json!(""))
            .with("platform", json!("MacIntel"))
            .with("oscpu", Value::Null)
            .with("language", json!("en-GB"))
            .with("languages", json!(["en-GB", "en"]))
            .with("hardwareConcurrency", json!(10))
            .with("deviceMemory", json!(8))
            .with("maxTouchPoints", json!(0))
            .with("webdriver", json!(false))
            .with("cookieEnabled", json!(true))
            .with("onLine", json!(true))
            .with("doNotTrack", Value::Null)
            .with("pdfViewerEnabled", json!(true));

        let screen = Interface::new("Screen", "Screen", "screen")
            .with("width", json!(1728))
            .with("height", json!(1117))
            .with("availWidth", json!(1728))
            .with("availHeight", json!(1080))
            .with("availLeft", json!(0))
            .with("availTop", json!(37))
            .with("colorDepth", json!(30))
            .with("pixelDepth", json!(30));

        let window = Interface::new("Window", "Window", "globalThis")
            .with("innerWidth", json!(1512))
            .with("innerHeight", json!(944))
            .with("outerWidth", json!(1512))
            .with("outerHeight", json!(1012))
            .with("screenX", json!(0))
            .with("screenY", json!(37))
            .with("devicePixelRatio", json!(2.0))
            .with("scrollX", json!(0))
            .with("scrollY", json!(0));

        Self {
            interfaces: vec![navigator, screen, window],
            plugins: vec![
                Plugin {
                    name: "PDF Viewer".to_string(),
                    filename: "internal-pdf-viewer".to_string(),
                    description: "Portable Document Format".to_string(),
                    mime_types: vec!["application/pdf".to_string(), "text/pdf".to_string()],
                },
                Plugin {
                    name: "Chrome PDF Viewer".to_string(),
                    filename: "internal-pdf-viewer".to_string(),
                    description: "Portable Document Format".to_string(),
                    mime_types: vec!["application/pdf".to_string(), "text/pdf".to_string()],
                },
                Plugin {
                    name: "Chromium PDF Viewer".to_string(),
                    filename: "internal-pdf-viewer".to_string(),
                    description: "Portable Document Format".to_string(),
                    mime_types: vec!["application/pdf".to_string(), "text/pdf".to_string()],
                },
                Plugin {
                    name: "Microsoft Edge PDF Viewer".to_string(),
                    filename: "internal-pdf-viewer".to_string(),
                    description: "Portable Document Format".to_string(),
                    mime_types: vec!["application/pdf".to_string(), "text/pdf".to_string()],
                },
                Plugin {
                    name: "WebKit built-in PDF".to_string(),
                    filename: "internal-pdf-viewer".to_string(),
                    description: "Portable Document Format".to_string(),
                    mime_types: vec!["application/pdf".to_string(), "text/pdf".to_string()],
                },
            ],
            webgl_parameters: BTreeMap::from([
                ("37445".to_string(), json!("Google Inc. (Apple)")),
                (
                    "37446".to_string(),
                    json!("ANGLE (Apple, ANGLE Metal Renderer: Apple M1 Pro, Unspecified Version)"),
                ),
                ("7936".to_string(), json!("WebKit")),
                ("7937".to_string(), json!("WebKit WebGL")),
                ("7938".to_string(), json!("WebGL 1.0 (OpenGL ES 2.0 Chromium)")),
                ("3379".to_string(), json!(16384)),
                ("34930".to_string(), json!(16)),
                ("36347".to_string(), json!(1024)),
                ("36348".to_string(), json!(32)),
            ]),
            webgl_extensions: vec![
                "ANGLE_instanced_arrays".to_string(),
                "EXT_blend_minmax".to_string(),
                "EXT_color_buffer_half_float".to_string(),
                "EXT_float_blend".to_string(),
                "EXT_texture_filter_anisotropic".to_string(),
                "OES_element_index_uint".to_string(),
                "OES_standard_derivatives".to_string(),
                "OES_texture_float".to_string(),
                "OES_vertex_array_object".to_string(),
                "WEBGL_debug_renderer_info".to_string(),
                "WEBGL_lose_context".to_string(),
            ],
            media_support: BTreeMap::from([
                ("video/mp4; codecs=\"avc1.42E01E\"".to_string(), "probably".to_string()),
                ("video/mp4; codecs=\"avc1.640028\"".to_string(), "probably".to_string()),
                ("video/webm; codecs=\"vp8\"".to_string(), "probably".to_string()),
                ("video/webm; codecs=\"vp9\"".to_string(), "probably".to_string()),
                ("video/webm; codecs=\"vp8, vorbis\"".to_string(), "probably".to_string()),
                ("audio/mpeg".to_string(), "probably".to_string()),
                ("audio/aac".to_string(), "probably".to_string()),
                ("audio/mp4; codecs=\"mp4a.40.2\"".to_string(), "probably".to_string()),
                ("audio/webm; codecs=\"vorbis\"".to_string(), "probably".to_string()),
                ("audio/webm; codecs=\"opus\"".to_string(), "probably".to_string()),
                ("audio/ogg; codecs=\"vorbis\"".to_string(), "probably".to_string()),
                ("video/ogg; codecs=\"theora\"".to_string(), String::new()),
            ]),
            media_queries: BTreeMap::from([
                ("(prefers-color-scheme: dark)".to_string(), true),
                ("(prefers-reduced-motion: reduce)".to_string(), false),
                ("(hover: hover)".to_string(), true),
                ("(pointer: fine)".to_string(), true),
                ("(pointer: coarse)".to_string(), false),
                ("(any-pointer: coarse)".to_string(), false),
                ("(color-gamut: p3)".to_string(), true),
                ("(forced-colors: active)".to_string(), false),
                ("(orientation: portrait)".to_string(), false),
                ("(orientation: landscape)".to_string(), true),
                ("(max-width: 767px)".to_string(), false),
                ("(min-width: 768px)".to_string(), true),
                ("(display-mode: browser)".to_string(), true),
                ("(scripting: enabled)".to_string(), true),
            ]),
            permissions: BTreeMap::from([
                ("notifications".to_string(), "default".to_string()),
                ("geolocation".to_string(), "prompt".to_string()),
                ("camera".to_string(), "prompt".to_string()),
                ("microphone".to_string(), "prompt".to_string()),
                ("midi".to_string(), "granted".to_string()),
                ("push".to_string(), "prompt".to_string()),
                ("persistent-storage".to_string(), "prompt".to_string()),
                ("clipboard-read".to_string(), "prompt".to_string()),
                ("clipboard-write".to_string(), "granted".to_string()),
                ("background-sync".to_string(), "granted".to_string()),
                ("accelerometer".to_string(), "granted".to_string()),
                ("gyroscope".to_string(), "granted".to_string()),
                ("magnetometer".to_string(), "granted".to_string()),
                ("speaker".to_string(), "invalid".to_string()),
                ("device-info".to_string(), "invalid".to_string()),
                ("bluetooth".to_string(), "invalid".to_string()),
                ("ambient-light-sensor".to_string(), "invalid".to_string()),
                ("clipboard".to_string(), "invalid".to_string()),
                ("accessibility-events".to_string(), "invalid".to_string()),
            ]),
            font_widths: BTreeMap::from([
                ("Arial".to_string(), 87.5),
                ("Courier New".to_string(), 105.0),
                ("Helvetica".to_string(), 87.5),
                ("Times New Roman".to_string(), 82.234375),
                ("Comic Sans MS".to_string(), 94.703125),
            ]),
            font_probe: "72px mmmmmmmmmmlli".to_string(),
            layout: BTreeMap::new(),
            canvas: BTreeMap::new(),
            mime_types: vec![
                MimeType {
                    kind: "application/pdf".to_string(),
                    suffixes: "pdf".to_string(),
                    description: "Portable Document Format".to_string(),
                    plugin: "PDF Viewer".to_string(),
                },
                MimeType {
                    kind: "text/pdf".to_string(),
                    suffixes: "pdf".to_string(),
                    description: "Portable Document Format".to_string(),
                    plugin: "PDF Viewer".to_string(),
                },
            ],
            user_agent_data: Some(UserAgentData {
                brands: vec![
                    Brand { brand: "Chromium".to_string(), version: "140".to_string() },
                    Brand { brand: "Not=A?Brand".to_string(), version: "24".to_string() },
                    Brand { brand: "Google Chrome".to_string(), version: "140".to_string() },
                ],
                mobile: false,
                platform: "macOS".to_string(),
                high_entropy: BTreeMap::from([
                    ("architecture".to_string(), json!("arm")),
                    ("bitness".to_string(), json!("64")),
                    ("model".to_string(), json!("")),
                    ("platformVersion".to_string(), json!("15.6.0")),
                    ("uaFullVersion".to_string(), json!("140.0.7339.133")),
                    ("wow64".to_string(), json!(false)),
                    (
                        "fullVersionList".to_string(),
                        json!([
                            { "brand": "Chromium", "version": "140.0.7339.133" },
                            { "brand": "Not=A?Brand", "version": "24.0.0.0" },
                            { "brand": "Google Chrome", "version": "140.0.7339.133" }
                        ]),
                    ),
                ]),
            }),
            connection: Some(Connection {
                downlink: 10.0,
                effective_type: "4g".to_string(),
                rtt: 50.0,
                save_data: false,
            }),
            intl: Some(Intl {
                locale: "en-GB".to_string(),
                calendar: "gregory".to_string(),
                numbering_system: "latn".to_string(),
                time_zone: "Europe/London".to_string(),
                hour_cycle: "h23".to_string(),
                timezone_offset: 0,
            }),
            webgl2_parameters: BTreeMap::new(),
            webgl2_extensions: Vec::new(),
            audio: Some(Audio {
                sample_rate: 48000.0,
                state: "suspended".to_string(),
                base_latency: 0.005333333333333333,
                output_latency: 0.0,
                max_channel_count: 2,
                channel_count: 2,
                rendered: String::new(),
            }),
            battery: Some(Battery {
                charging: true,
                level: 1.0,
                charging_time: Some(0.0),
                discharging_time: None,
            }),
            voices: Vec::new(),
            media_devices: Vec::new(),
            memory: Some(Memory {
                js_heap_size_limit: 4_294_705_152.0,
                total_js_heap_size: 12_000_000.0,
                used_js_heap_size: 10_000_000.0,
            }),
            orientation: Some(Orientation {
                angle: 0.0,
                kind: "landscape-primary".to_string(),
            }),
            chrome: Some(json!({ "runtime": {}, "app": { "isInstalled": false } })),
            document: BTreeMap::from([
                ("characterSet".to_string(), json!("UTF-8")),
                ("charset".to_string(), json!("UTF-8")),
                ("contentType".to_string(), json!("text/html")),
                ("compatMode".to_string(), json!("CSS1Compat")),
                ("designMode".to_string(), json!("off")),
                ("dir".to_string(), json!("")),
                ("inputEncoding".to_string(), json!("UTF-8")),
                ("visibilityState".to_string(), json!("visible")),
                ("hidden".to_string(), json!(false)),
            ]),
            window_order: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_profile_validates_and_carries_the_three_interfaces() {
        let profile = Profile::desktop_chrome();

        profile.validate().unwrap();
        assert!(profile.interface("Navigator").is_some());
        assert!(profile.interface("Screen").is_some());
        assert!(profile.interface("Window").is_some());
    }

    #[test]
    fn the_default_profile_is_internally_consistent() {
        let profile = Profile::desktop_chrome();

        let agent = profile.property("Navigator", "userAgent").unwrap().as_str().unwrap();
        let platform = profile.property("Navigator", "platform").unwrap().as_str().unwrap();

        assert!(agent.contains("Macintosh"), "{agent}");
        assert_eq!(platform, "MacIntel");
        assert_eq!(profile.property("Navigator", "webdriver"), Some(&json!(false)));

        let inner = profile.property("Window", "innerHeight").unwrap().as_f64().unwrap();
        let outer = profile.property("Window", "outerHeight").unwrap().as_f64().unwrap();
        let screen = profile.property("Screen", "height").unwrap().as_f64().unwrap();

        assert!(inner < outer, "the viewport must be shorter than the window");
        assert!(outer <= screen, "the window must fit on the screen");
    }

    #[test]
    fn the_default_profile_has_plugins_and_a_real_renderer() {
        let profile = Profile::desktop_chrome();

        assert_eq!(profile.plugins.len(), 5);
        assert!(profile.webgl_extensions.contains(&"WEBGL_debug_renderer_info".to_string()));

        let renderer = profile.webgl_parameters.get("37446").unwrap().as_str().unwrap();
        assert!(!renderer.contains("SwiftShader"), "{renderer}");
    }

    #[test]
    fn a_property_can_be_changed_and_an_unknown_interface_is_rejected() {
        let mut profile = Profile::desktop_chrome();

        profile.set("Navigator", "hardwareConcurrency", json!(4)).unwrap();
        assert_eq!(profile.property("Navigator", "hardwareConcurrency"), Some(&json!(4)));

        assert!(profile.set("Nonsense", "x", json!(1)).is_err());
    }

    #[test]
    fn duplicate_brands_are_rejected() {
        let mut profile = Profile::desktop_chrome();
        profile.interfaces.push(Interface::new("Navigator", "Navigator", "navigator"));

        assert!(profile.validate().unwrap_err().to_string().contains("share the brand"));
    }

    #[test]
    fn a_profile_round_trips_through_json() {
        let profile = Profile::desktop_chrome();
        let text = serde_json::to_string(&profile).unwrap();
        assert_eq!(serde_json::from_str::<Profile>(&text).unwrap(), profile);
    }
}
