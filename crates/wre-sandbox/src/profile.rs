use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use wre_core::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plugin {
    pub name: String,
    pub filename: String,
    pub description: String,
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
    pub webgl_parameters: BTreeMap<String, Value>,
    #[serde(default)]
    pub webgl_extensions: Vec<String>,
    #[serde(default)]
    pub media_support: BTreeMap<String, String>,
    #[serde(default)]
    pub media_queries: BTreeMap<String, bool>,
    #[serde(default)]
    pub permissions: BTreeMap<String, String>,
    #[serde(default)]
    pub font_widths: BTreeMap<String, f64>,
    #[serde(default)]
    pub layout: BTreeMap<String, Value>,
    #[serde(default)]
    pub canvas: BTreeMap<String, String>,
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
                },
                Plugin {
                    name: "Chrome PDF Viewer".to_string(),
                    filename: "internal-pdf-viewer".to_string(),
                    description: "Portable Document Format".to_string(),
                },
                Plugin {
                    name: "Chromium PDF Viewer".to_string(),
                    filename: "internal-pdf-viewer".to_string(),
                    description: "Portable Document Format".to_string(),
                },
                Plugin {
                    name: "Microsoft Edge PDF Viewer".to_string(),
                    filename: "internal-pdf-viewer".to_string(),
                    description: "Portable Document Format".to_string(),
                },
                Plugin {
                    name: "WebKit built-in PDF".to_string(),
                    filename: "internal-pdf-viewer".to_string(),
                    description: "Portable Document Format".to_string(),
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
                ("video/webm; codecs=\"vp8\"".to_string(), "probably".to_string()),
                ("video/webm; codecs=\"vp9\"".to_string(), "probably".to_string()),
                ("audio/mpeg".to_string(), "probably".to_string()),
                ("audio/ogg; codecs=\"vorbis\"".to_string(), "probably".to_string()),
                ("video/ogg; codecs=\"theora\"".to_string(), String::new()),
            ]),
            media_queries: BTreeMap::from([
                ("(prefers-color-scheme: dark)".to_string(), true),
                ("(prefers-reduced-motion: reduce)".to_string(), false),
                ("(hover: hover)".to_string(), true),
                ("(pointer: fine)".to_string(), true),
                ("(any-pointer: coarse)".to_string(), false),
                ("(color-gamut: p3)".to_string(), true),
                ("(forced-colors: active)".to_string(), false),
            ]),
            permissions: BTreeMap::from([
                ("notifications".to_string(), "default".to_string()),
                ("geolocation".to_string(), "prompt".to_string()),
                ("camera".to_string(), "prompt".to_string()),
                ("microphone".to_string(), "prompt".to_string()),
            ]),
            font_widths: BTreeMap::from([
                ("Arial".to_string(), 87.5),
                ("Courier New".to_string(), 105.0),
                ("Helvetica".to_string(), 87.5),
                ("Times New Roman".to_string(), 82.234375),
                ("Comic Sans MS".to_string(), 94.703125),
            ]),
            layout: BTreeMap::new(),
            canvas: BTreeMap::new(),
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
