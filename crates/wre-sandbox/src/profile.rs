use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use wre_core::error::{Error, Result};

static BUNDLED_SURFACE: &str = include_str!("../assets/desktop-chrome.json");

const BUNDLED_CHROME: &str = "151";

static BUNDLED: std::sync::LazyLock<Profile> = std::sync::LazyLock::new(|| {
    serde_json::from_str(BUNDLED_SURFACE).expect("the bundled surface parses")
});

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
    #[serde(default)]
    pub rendered_sum: Option<f64>,
    #[serde(default)]
    pub reduction: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StorageEstimate {
    pub quota: f64,
    #[serde(default)]
    pub usage: f64,
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
    pub storage: Option<StorageEstimate>,
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

    pub fn retune_chrome(&mut self, version: &str) {
        let Some(current) = self.chrome_version() else {
            return;
        };

        if current == version {
            return;
        }

        let swap = |text: &str| text.replace(&format!("Chrome/{current}."), &format!("Chrome/{version}."));

        for brand in ["Navigator", "WorkerNavigator"] {
            for name in ["userAgent", "appVersion"] {
                let Some(found) = self.property(brand, name).and_then(Value::as_str).map(swap) else {
                    continue;
                };

                let _ = self.set(brand, name, Value::String(found));
            }
        }

        if let Some(data) = self.user_agent_data.as_mut() {
            for entry in &mut data.brands {
                if entry.version == current {
                    entry.version = version.to_string();
                }
            }

            retune_entropy(&mut data.high_entropy, &current, version);
        }
    }

    pub fn chrome_version(&self) -> Option<String> {
        self.property("Navigator", "userAgent")
            .and_then(Value::as_str)?
            .split("Chrome/")
            .nth(1)?
            .split('.')
            .next()
            .map(str::to_string)
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
        let mut profile = BUNDLED.clone();
        profile.retune_chrome(BUNDLED_CHROME);
        profile
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

fn retune_entropy(entropy: &mut BTreeMap<String, Value>, current: &str, version: &str) {
    let swap_version = |value: &mut Value| {
        let Some(text) = value.as_str() else {
            return;
        };

        if text.split('.').next() == Some(current) {
            let rest: Vec<&str> = text.split('.').skip(1).collect();
            let joined = if rest.is_empty() { version.to_string() } else { format!("{version}.{}", rest.join(".")) };
            *value = Value::String(joined);
        }
    };

    if let Some(found) = entropy.get_mut("uaFullVersion") {
        swap_version(found);
    }

    if let Some(Value::Array(list)) = entropy.get_mut("fullVersionList") {
        for entry in list {
            if let Some(found) = entry.get_mut("version") {
                swap_version(found);
            }
        }
    }
}
