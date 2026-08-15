use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wreq::IntoEmulation;
use wreq::header::USER_AGENT;

use wre_core::error::{Error, Result};

pub use wreq_util::{Platform, Profile};

/// Transport identity for an HTTP client: the TLS handshake, HTTP/2 settings and
/// default headers of one browser build on one platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fingerprint {
    pub profile: Profile,
    pub platform: Platform,
    #[serde(default = "enabled")]
    pub http2: bool,
    #[serde(default = "enabled")]
    pub headers: bool,
}

fn enabled() -> bool {
    true
}

impl Default for Fingerprint {
    fn default() -> Self {
        Self {
            profile: Profile::Chrome140,
            platform: Platform::MacOS,
            http2: true,
            headers: true,
        }
    }
}

impl Fingerprint {
    pub fn new(profile: Profile, platform: Platform) -> Self {
        Self { profile, platform, ..Self::default() }
    }

    /// Picks the profile and platform closest to what `agent` claims, so the TLS and
    /// HTTP/2 layers tell the same story as the `User-Agent` header.
    pub fn from_user_agent(agent: &str) -> Option<Self> {
        let platform = platform_of(agent);
        let family = family_of(agent, platform)?;
        let wanted = version_key(&version_of(agent, family));

        let profile = Profile::VARIANTS
            .iter()
            .copied()
            .filter_map(|profile| {
                let name = profile_name(profile);
                let (candidate, version) = split_name(&name)?;
                (candidate == family).then(|| (profile, version_key(&version)))
            })
            .min_by_key(|(_, key)| wanted.abs_diff(*key))
            .map(|(profile, _)| profile)?;

        Some(Self { profile, platform, ..Self::default() })
    }

    /// The `User-Agent` this fingerprint sends by default.
    pub fn user_agent(&self) -> Option<String> {
        let emulation = wreq_util::Emulation::builder()
            .profile(self.profile)
            .platform(self.platform)
            .http2(self.http2)
            .headers(true)
            .build()
            .into_emulation();

        emulation
            .headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    }

    pub fn profile_name(&self) -> String {
        profile_name(self.profile)
    }

    pub fn platform_name(&self) -> String {
        platform_name(self.platform)
    }

    pub fn with_platform(mut self, platform: Platform) -> Self {
        self.platform = platform;
        self
    }

    pub fn without_headers(mut self) -> Self {
        self.headers = false;
        self
    }

    pub fn without_http2(mut self) -> Self {
        self.http2 = false;
        self
    }
}

impl IntoEmulation for Fingerprint {
    fn into_emulation(self) -> wreq::Emulation {
        wreq_util::Emulation::builder()
            .profile(self.profile)
            .platform(self.platform)
            .http2(self.http2)
            .headers(self.headers)
            .build()
            .into_emulation()
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.profile_name(), self.platform_name())
    }
}

impl FromStr for Fingerprint {
    type Err = Error;

    fn from_str(spec: &str) -> Result<Self> {
        let (profile, platform) = match spec.split_once([':', '/']) {
            Some((profile, platform)) => (profile.trim(), Some(platform.trim())),
            None => (spec.trim(), None),
        };

        let profile = parse_profile(profile)?;
        let platform = match platform {
            Some(name) => parse_platform(name)?,
            None => default_platform(profile),
        };

        Ok(Self::new(profile, platform))
    }
}

pub fn profile_name(profile: Profile) -> String {
    serde_json::to_value(profile)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{profile:?}").to_lowercase())
}

pub fn platform_name(platform: Platform) -> String {
    serde_json::to_value(platform)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{platform:?}").to_lowercase())
}

pub fn parse_profile(name: &str) -> Result<Profile> {
    let normalized = name.trim().to_lowercase().replace(['-', ' '], "_");
    serde_json::from_value(Value::String(normalized)).map_err(|_| {
        Error::msg(format!(
            "unknown client profile {name}, expected a wreq-util profile name such as chrome_140, firefox_146 or safari_18.5"
        ))
    })
}

pub fn parse_platform(name: &str) -> Result<Platform> {
    let normalized = name.trim().to_lowercase();
    serde_json::from_value(Value::String(normalized)).map_err(|_| {
        Error::msg(format!("unknown platform {name}, expected windows, macos, linux, android or ios"))
    })
}

pub fn profile_names() -> Vec<String> {
    Profile::VARIANTS.iter().copied().map(profile_name).collect()
}

fn default_platform(profile: Profile) -> Platform {
    let name = profile_name(profile);
    if name.contains("ios") || name.contains("ipad") {
        Platform::IOS
    } else if name.contains("android") || name.starts_with("okhttp") {
        Platform::Android
    } else if name.starts_with("safari") {
        Platform::MacOS
    } else {
        Platform::default()
    }
}

fn split_name(name: &str) -> Option<(String, Vec<u32>)> {
    let (family, version) = name.rsplit_once('_')?;
    let parts: Vec<u32> = version.split('.').filter_map(|part| part.parse().ok()).collect();
    if parts.is_empty() { None } else { Some((family.to_string(), parts)) }
}

fn version_key(parts: &[u32]) -> u64 {
    let major = parts.first().copied().unwrap_or(0) as u64;
    let minor = parts.get(1).copied().unwrap_or(0) as u64;
    let patch = parts.get(2).copied().unwrap_or(0) as u64;
    major * 1_000_000 + minor.min(999) * 1_000 + patch.min(999)
}

fn platform_of(agent: &str) -> Platform {
    if agent.contains("Android") {
        Platform::Android
    } else if agent.contains("iPhone") || agent.contains("iPad") || agent.contains("iPod") {
        Platform::IOS
    } else if agent.contains("Macintosh") || agent.contains("Mac OS X") {
        Platform::MacOS
    } else if agent.contains("Windows") {
        Platform::Windows
    } else if agent.contains("Linux") || agent.contains("X11") {
        Platform::Linux
    } else {
        Platform::default()
    }
}

fn family_of(agent: &str, platform: Platform) -> Option<&'static str> {
    if agent.starts_with("okhttp/") {
        return Some("okhttp");
    }
    if agent.contains("Edg/") || agent.contains("Edge/") {
        return Some("edge");
    }
    if agent.contains("OPR/") || agent.contains("Opera") {
        return Some("opera");
    }
    if agent.contains("Firefox/") {
        return Some(match platform {
            Platform::Android => "firefox_android",
            _ => "firefox",
        });
    }
    if agent.contains("Chrome/") || agent.contains("CriOS/") {
        return Some("chrome");
    }
    if agent.contains("Safari/") && agent.contains("Version/") {
        return Some(if agent.contains("iPad") {
            "safari_ipad"
        } else if agent.contains("iPhone") || agent.contains("iPod") {
            "safari_ios"
        } else {
            "safari"
        });
    }
    None
}

fn version_of(agent: &str, family: &str) -> Vec<u32> {
    let token = match family {
        "okhttp" => "okhttp/",
        "edge" => {
            if agent.contains("Edg/") {
                "Edg/"
            } else {
                "Edge/"
            }
        }
        "opera" => {
            if agent.contains("OPR/") {
                "OPR/"
            } else {
                "Opera/"
            }
        }
        "firefox" | "firefox_android" => "Firefox/",
        "safari" | "safari_ios" | "safari_ipad" => "Version/",
        _ => {
            if agent.contains("CriOS/") {
                "CriOS/"
            } else {
                "Chrome/"
            }
        }
    };

    let Some(start) = agent.find(token) else {
        return Vec::new();
    };

    let rest = &agent[start + token.len()..];
    let end = rest
        .find(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .unwrap_or(rest.len());

    rest[..end]
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_chrome_agent_to_matching_profile() {
        let agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
        let fingerprint = Fingerprint::from_user_agent(agent).unwrap();
        assert_eq!(fingerprint.profile_name(), "chrome_140");
        assert_eq!(fingerprint.platform, Platform::Windows);
        assert_eq!(fingerprint.user_agent().as_deref(), Some(agent));
    }

    #[test]
    fn maps_edge_before_chrome() {
        let agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36 Edg/136.0.0.0";
        let fingerprint = Fingerprint::from_user_agent(agent).unwrap();
        assert_eq!(fingerprint.profile_name(), "edge_136");
    }

    #[test]
    fn maps_ios_safari_to_ios_profile() {
        let agent = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1.1 Mobile/15E148 Safari/604.1";
        let fingerprint = Fingerprint::from_user_agent(agent).unwrap();
        assert_eq!(fingerprint.profile_name(), "safari_ios_18.1.1");
        assert_eq!(fingerprint.platform, Platform::IOS);
    }

    #[test]
    fn falls_back_to_nearest_known_version() {
        let agent = "Mozilla/5.0 (X11; Linux x86_64) Gecko/20100101 Firefox/102.0";
        let fingerprint = Fingerprint::from_user_agent(agent).unwrap();
        assert_eq!(fingerprint.profile_name(), "firefox_109");
        assert_eq!(fingerprint.platform, Platform::Linux);
    }

    #[test]
    fn parses_spec_with_and_without_platform() {
        let explicit: Fingerprint = "chrome_141:windows".parse().unwrap();
        assert_eq!(explicit.profile_name(), "chrome_141");
        assert_eq!(explicit.platform, Platform::Windows);

        let implied: Fingerprint = "safari_ios_26".parse().unwrap();
        assert_eq!(implied.platform, Platform::IOS);

        assert!("netscape_4".parse::<Fingerprint>().is_err());
    }

    #[test]
    fn round_trips_display_form() {
        let fingerprint = Fingerprint::default();
        assert_eq!(fingerprint.to_string(), "chrome_140:macos");
        assert_eq!(fingerprint.to_string().parse::<Fingerprint>().unwrap(), fingerprint);
    }
}
