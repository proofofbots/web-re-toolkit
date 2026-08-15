use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use wre_core::bundle::EmulationEntry;
use wre_core::error::Result;

use crate::session::Session;

pub const REALISTIC_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfile {
    pub width: u32,
    pub height: u32,
    pub screen_width: u32,
    pub screen_height: u32,
    pub device_scale_factor: f64,
    pub mobile: bool,
    pub user_agent: String,
    pub platform: String,
    pub accept_language: String,
    pub timezone: String,
    pub touch_points: u32,
    pub hardware_concurrency: Option<u32>,
}

impl Default for DeviceProfile {
    fn default() -> Self {
        Self {
            width: 1512,
            height: 895,
            screen_width: 1512,
            screen_height: 982,
            device_scale_factor: 2.0,
            mobile: false,
            user_agent: REALISTIC_UA.to_string(),
            platform: "MacIntel".to_string(),
            accept_language: "en-US,en;q=0.9".to_string(),
            timezone: "America/New_York".to_string(),
            touch_points: 0,
            hardware_concurrency: Some(8),
        }
    }
}

impl DeviceProfile {
    pub fn windows() -> Self {
        Self {
            width: 1920,
            height: 945,
            screen_width: 1920,
            screen_height: 1080,
            device_scale_factor: 1.0,
            user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36".to_string(),
            platform: "Win32".to_string(),
            ..Self::default()
        }
    }

    pub fn entries(&self) -> Vec<EmulationEntry> {
        let mut out = vec![
            EmulationEntry {
                method: "Emulation.setDeviceMetricsOverride".to_string(),
                params: json!({
                    "width": self.width,
                    "height": self.height,
                    "deviceScaleFactor": self.device_scale_factor,
                    "mobile": self.mobile,
                    "screenWidth": self.screen_width,
                    "screenHeight": self.screen_height,
                    "positionX": 0,
                    "positionY": 0,
                }),
            },
            EmulationEntry {
                method: "Emulation.setUserAgentOverride".to_string(),
                params: json!({
                    "userAgent": self.user_agent,
                    "acceptLanguage": self.accept_language,
                    "platform": self.platform,
                }),
            },
            EmulationEntry {
                method: "Emulation.setTimezoneOverride".to_string(),
                params: json!({ "timezoneId": self.timezone }),
            },
            EmulationEntry {
                method: "Emulation.setTouchEmulationEnabled".to_string(),
                params: json!({ "enabled": self.touch_points > 0, "maxTouchPoints": self.touch_points }),
            },
            EmulationEntry {
                method: "Emulation.setFocusEmulationEnabled".to_string(),
                params: json!({ "enabled": true }),
            },
        ];

        if let Some(cores) = self.hardware_concurrency {
            out.push(EmulationEntry {
                method: "Emulation.setHardwareConcurrencyOverride".to_string(),
                params: json!({ "hardwareConcurrency": cores }),
            });
        }

        out
    }
}

pub async fn apply(session: &Session, entries: &[EmulationEntry]) -> Vec<String> {
    let mut failures = Vec::new();

    for entry in entries {
        if session.try_send(&entry.method, entry.params.clone()).await.is_none() {
            failures.push(entry.method.clone());
        }
    }

    failures
}

pub async fn reset(session: &Session) -> Result<()> {
    for method in [
        "Emulation.clearDeviceMetricsOverride",
        "Emulation.clearGeolocationOverride",
        "Network.clearAcceptedEncodingsOverride",
    ] {
        session.try_send(method, json!({})).await;
    }

    session
        .try_send("Emulation.setUserAgentOverride", json!({ "userAgent": REALISTIC_UA }))
        .await;

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Knob {
    pub name: String,
    pub group: String,
    pub entries: Vec<EmulationEntry>,
}

impl Knob {
    pub fn new(name: &str, group: &str, entries: Vec<EmulationEntry>) -> Self {
        Self { name: name.to_string(), group: group.to_string(), entries }
    }

    fn single(name: &str, group: &str, method: &str, params: Value) -> Self {
        Self::new(
            name,
            group,
            vec![EmulationEntry { method: method.to_string(), params }],
        )
    }
}

pub fn standard_knobs(base: &DeviceProfile) -> Vec<Knob> {
    let mut knobs = Vec::new();

    knobs.push(Knob::single(
        "screen-1280x800",
        "screen",
        "Emulation.setDeviceMetricsOverride",
        json!({
            "width": 1280, "height": 720, "deviceScaleFactor": base.device_scale_factor,
            "mobile": false, "screenWidth": 1280, "screenHeight": 800,
        }),
    ));

    knobs.push(Knob::single(
        "screen-1920x1080",
        "screen",
        "Emulation.setDeviceMetricsOverride",
        json!({
            "width": 1920, "height": 945, "deviceScaleFactor": base.device_scale_factor,
            "mobile": false, "screenWidth": 1920, "screenHeight": 1080,
        }),
    ));

    knobs.push(Knob::single(
        "dpr-1",
        "screen",
        "Emulation.setDeviceMetricsOverride",
        json!({
            "width": base.width, "height": base.height, "deviceScaleFactor": 1.0,
            "mobile": false, "screenWidth": base.screen_width, "screenHeight": base.screen_height,
        }),
    ));

    knobs.push(Knob::single(
        "dpr-3",
        "screen",
        "Emulation.setDeviceMetricsOverride",
        json!({
            "width": base.width, "height": base.height, "deviceScaleFactor": 3.0,
            "mobile": false, "screenWidth": base.screen_width, "screenHeight": base.screen_height,
        }),
    ));

    for zone in ["Europe/Berlin", "Asia/Tokyo", "Australia/Sydney"] {
        knobs.push(Knob::single(
            &format!("timezone-{}", zone.replace('/', "-").to_lowercase()),
            "locale",
            "Emulation.setTimezoneOverride",
            json!({ "timezoneId": zone }),
        ));
    }

    for locale in ["de-DE", "ja-JP", "pt-BR"] {
        knobs.push(Knob::new(
            &format!("locale-{}", locale.to_lowercase()),
            "locale",
            vec![
                EmulationEntry {
                    method: "Emulation.setLocaleOverride".to_string(),
                    params: json!({ "locale": locale }),
                },
                EmulationEntry {
                    method: "Emulation.setUserAgentOverride".to_string(),
                    params: json!({
                        "userAgent": base.user_agent,
                        "acceptLanguage": format!("{locale},en;q=0.8"),
                        "platform": base.platform,
                    }),
                },
            ],
        ));
    }

    knobs.push(Knob::single(
        "ua-windows",
        "agent",
        "Emulation.setUserAgentOverride",
        json!({
            "userAgent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
            "acceptLanguage": base.accept_language,
            "platform": "Win32",
        }),
    ));

    knobs.push(Knob::single(
        "ua-linux",
        "agent",
        "Emulation.setUserAgentOverride",
        json!({
            "userAgent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
            "acceptLanguage": base.accept_language,
            "platform": "Linux x86_64",
        }),
    ));

    for cores in [2u32, 4, 16] {
        knobs.push(Knob::single(
            &format!("cores-{cores}"),
            "hardware",
            "Emulation.setHardwareConcurrencyOverride",
            json!({ "hardwareConcurrency": cores }),
        ));
    }

    knobs.push(Knob::single(
        "touch-5",
        "hardware",
        "Emulation.setTouchEmulationEnabled",
        json!({ "enabled": true, "maxTouchPoints": 5 }),
    ));

    knobs.push(Knob::single(
        "cpu-throttle-4x",
        "performance",
        "Emulation.setCPUThrottlingRate",
        json!({ "rate": 4 }),
    ));

    knobs.push(Knob::single(
        "prefers-dark",
        "media",
        "Emulation.setEmulatedMedia",
        json!({ "features": [{ "name": "prefers-color-scheme", "value": "dark" }] }),
    ));

    knobs.push(Knob::single(
        "prefers-reduced-motion",
        "media",
        "Emulation.setEmulatedMedia",
        json!({ "features": [{ "name": "prefers-reduced-motion", "value": "reduce" }] }),
    ));

    knobs.push(Knob::single(
        "media-print",
        "media",
        "Emulation.setEmulatedMedia",
        json!({ "media": "print" }),
    ));

    knobs.push(Knob::single(
        "unfocused",
        "focus",
        "Emulation.setFocusEmulationEnabled",
        json!({ "enabled": false }),
    ));

    knobs.push(Knob::single(
        "geolocation-tokyo",
        "locale",
        "Emulation.setGeolocationOverride",
        json!({ "latitude": 35.6762, "longitude": 139.6503, "accuracy": 20 }),
    ));

    knobs.push(Knob::single(
        "offline",
        "network",
        "Network.emulateNetworkConditions",
        json!({ "offline": false, "latency": 400, "downloadThroughput": 200_000, "uploadThroughput": 100_000 }),
    ));

    knobs
}

pub fn knob_by_name<'k>(knobs: &'k [Knob], name: &str) -> Option<&'k Knob> {
    knobs.iter().find(|knob| knob.name == name)
}
