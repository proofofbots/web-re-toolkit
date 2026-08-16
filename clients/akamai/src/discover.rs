use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

static AKAM_SRC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)src\s*=\s*["']([^"']*/akam/(\d+)/([A-Za-z0-9_-]+)(?:\?[^"']*)?)["']"#)
        .expect("akam pattern")
});

static SCRIPT_SRC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<script[^>]*\ssrc\s*=\s*["']([^"']+)["']"#).expect("script pattern"));

static BAZA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)bazadebezolkohpepadr\s*=\s*["'](\d+)["']"#).expect("baza pattern")
});

static CHALLENGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(sec-cpt|_sec/cp_challenge|challenge_id|cp_challenge)"#).expect("challenge pattern")
});

static SEGMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_-]{2,24}$").expect("segment pattern"));

const MARK: &str = "aeiouy13579";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Sensor,
    Pixel,
    Obfuscated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Script {
    pub kind: Kind,
    pub url: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub generation: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Flags {
    pub force_secure: bool,
    pub bot_manager: bool,
    pub proof_of_work: bool,
    pub ip_reputation: bool,
    pub akid: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub segment: String,
    pub from_host: bool,
    pub bits: String,
    pub flags: Option<Flags>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Surface {
    pub sensor: Option<Script>,
    pub pixel_client: Option<Script>,
    pub pixel_post: Option<String>,
    pub baza: Option<String>,
    pub config: Option<Config>,
    pub scripts: Vec<Script>,
    pub challenge_page: bool,
}

impl Surface {
    pub fn is_protected(&self) -> bool {
        self.sensor.is_some() || self.pixel_client.is_some()
    }
}

pub fn discover(html: &str, base: &str) -> Surface {
    let mut akam = Vec::new();

    for found in AKAM_SRC.captures_iter(html) {
        let href = found.get(1).map_or("", |part| part.as_str());
        let generation = found
            .get(2)
            .and_then(|part| part.as_str().parse::<u32>().ok())
            .unwrap_or_default();
        let name = found.get(3).map_or("", |part| part.as_str()).to_string();
        let pixel = name.starts_with("pixel_");

        akam.push(Script {
            kind: if pixel { Kind::Pixel } else { Kind::Sensor },
            url: absolute(base, href),
            name,
            generation,
        });
    }

    let mut obfuscated = Vec::new();

    for found in SCRIPT_SRC.captures_iter(html) {
        let href = found.get(1).map_or("", |part| part.as_str());
        if !looks_obfuscated(href, base) {
            continue;
        }

        obfuscated.push(Script {
            kind: Kind::Obfuscated,
            url: absolute(base, href),
            name: String::new(),
            generation: 0,
        });
    }

    let plain_akam = akam.iter().find(|script| script.kind == Kind::Sensor).cloned();
    let sensor = obfuscated.first().cloned().or_else(|| plain_akam.clone());

    let pixel_client = match (&sensor, &plain_akam) {
        (Some(chosen), Some(plain)) if chosen.url != plain.url => Some(plain.clone()),
        _ => akam
            .iter()
            .find(|script| script.kind == Kind::Pixel)
            .and_then(|pixel| {
                let hash = pixel.name.trim_start_matches("pixel_").to_string();
                plain_akam
                    .clone()
                    .filter(|plain| plain.name == hash)
            }),
    };

    let baza = BAZA
        .captures(html)
        .and_then(|found| found.get(1))
        .map(|part| part.as_str().to_string());

    let pixel_post = match (&baza, &pixel_client) {
        (Some(seed), Some(client)) => pixel_post_url(&client.url, seed),
        _ => akam
            .iter()
            .find(|script| script.kind == Kind::Pixel)
            .map(|script| script.url.split('?').next().unwrap_or_default().to_string()),
    };

    let config = sensor.as_ref().and_then(|script| read_config(&script.url));

    let mut scripts = akam;
    scripts.extend(obfuscated);

    Surface {
        sensor,
        pixel_client,
        pixel_post,
        baza,
        config,
        scripts,
        challenge_page: CHALLENGE.is_match(html),
    }
}

pub fn pixel_hash(seed: &str) -> Option<String> {
    let value = seed.parse::<i64>().ok()?;
    Some(format!("{:x}", 77 ^ value))
}

fn pixel_post_url(client: &str, seed: &str) -> Option<String> {
    let hash = pixel_hash(seed)?;
    let base = client.rsplit_once('/')?.0;
    Some(format!("{base}/pixel_{hash}"))
}

pub fn read_config(script_url: &str) -> Option<Config> {
    let parts: Vec<&str> = script_url.split('/').collect();
    if parts.len() < 4 {
        return None;
    }

    let segment = parts[parts.len() - 4].to_string();
    if segment.is_empty() || segment.len() % 2 != 0 {
        return None;
    }

    let from_host = Url::parse(script_url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(|host| host == segment))
        .unwrap_or(false);

    let bits = bits_from(&segment);

    if bits.len() <= 3 {
        return Some(Config {
            segment,
            from_host,
            bits,
            flags: None,
            note: Some("too few bits, the config is not applied".to_string()),
        });
    }

    let bit = |index: usize| bits.as_bytes().get(index) == Some(&b'1');

    let flags = Flags {
        force_secure: bit(0),
        bot_manager: bit(1),
        proof_of_work: bit(2),
        ip_reputation: bit(3),
        akid: bits.len() > 4 && bit(4),
    };

    Some(Config { segment, from_host, bits, flags: Some(flags), note: None })
}

fn bits_from(segment: &str) -> String {
    let lower = segment.to_lowercase();
    let characters: Vec<char> = lower.chars().collect();
    let mut bits = String::new();

    let mut index = 0;
    while index < characters.len() {
        let first = MARK.contains(characters[index]);
        let second = characters
            .get(index + 1)
            .map(|found| MARK.contains(*found))
            .unwrap_or(false);

        bits.push(if first || second { '1' } else { '0' });
        index += 2;
    }

    bits
}

fn looks_obfuscated(href: &str, base: &str) -> bool {
    let path = if href.starts_with("http") {
        match Url::parse(href) {
            Ok(parsed) => parsed.path().to_string(),
            Err(_) => return false,
        }
    } else {
        href.split('?').next().unwrap_or_default().to_string()
    };

    if href.starts_with("http") {
        let (Ok(target), Ok(page)) = (Url::parse(href), Url::parse(base)) else {
            return false;
        };
        if target.host_str() != page.host_str() {
            return false;
        }
    }

    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();

    if segments.len() < 4 || path.contains("/akam/") {
        return false;
    }

    if let Some(last) = segments.last()
        && last.contains('.')
    {
        return false;
    }

    segments.iter().all(|segment| SEGMENT.is_match(segment))
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

    const MAERSK: &str = r#"
<html><head>
<script type="text/javascript">bazadebezolkohpepadr="1320943881"</script>
<script type="text/javascript" src="/akam/13/4ebc0144"></script>
<script type="text/javascript" src="/akam/13/pixel_4ebc0144?a=dD0xJmpzPW9mZg=="></script>
<script src="/pWSY7c1/2ib/AKfr/hFDsQoTHmWt/YwEfMkyRK8U/Y3g/AWJXAjJfBQ"></script>
</head><body></body></html>
"#;

    const PLAIN: &str = r#"<html><head>
<script src="https://www.example.com/akam/11/5c9e4a7b"></script>
</head></html>"#;

    #[test]
    fn the_obfuscated_path_is_the_sensor_and_the_akam_script_is_the_pixel_client() {
        let surface = discover(MAERSK, "https://www.maersk.com/tracking/ABC1234567");

        let sensor = surface.sensor.clone().expect("sensor");
        assert_eq!(sensor.kind, Kind::Obfuscated);
        assert!(sensor.url.ends_with("/pWSY7c1/2ib/AKfr/hFDsQoTHmWt/YwEfMkyRK8U/Y3g/AWJXAjJfBQ"));

        let pixel = surface.pixel_client.clone().expect("pixel client");
        assert_eq!(pixel.url, "https://www.maersk.com/akam/13/4ebc0144");
        assert_eq!(surface.baza.as_deref(), Some("1320943881"));
        assert_eq!(surface.pixel_post.as_deref(), Some("https://www.maersk.com/akam/13/pixel_4ebc0144"));
        assert!(surface.is_protected());
        assert!(!surface.challenge_page);
    }

    #[test]
    fn a_page_with_only_an_akam_script_uses_it_as_the_sensor() {
        let surface = discover(PLAIN, "https://www.example.com/");
        let sensor = surface.sensor.expect("sensor");

        assert_eq!(sensor.kind, Kind::Sensor);
        assert_eq!(sensor.generation, 11);
        assert_eq!(sensor.name, "5c9e4a7b");
        assert!(surface.pixel_client.is_none());
    }

    #[test]
    fn the_pixel_post_path_comes_from_the_seed() {
        assert_eq!(pixel_hash("1320943881").as_deref(), Some("4ebc0144"));
    }

    #[test]
    fn the_config_segment_decodes_to_the_flag_bits() {
        let config = read_config("https://www.example.com/aBcDeF/gHiJkLmN/mNoPqR/sTuVwX/yZ0123")
            .expect("config");

        assert_eq!(config.segment, "gHiJkLmN");
        assert_eq!(config.bits, "0100");
        assert!(!config.from_host);

        let flags = config.flags.expect("flags");
        assert!(flags.bot_manager);
        assert!(!flags.force_secure);
        assert!(!flags.proof_of_work);
        assert!(!flags.akid);
    }

    #[test]
    fn an_odd_segment_carries_no_config() {
        assert!(
            read_config("https://www.maersk.com/pWSY7c1/2ib/AKfr/hFDsQoTHmWt/YwEfMkyRK8U/Y3g/AWJXAjJfBQ")
                .is_none()
        );
    }

    #[test]
    fn a_challenge_page_is_called_out() {
        let surface = discover(
            r#"<html><body><form action="/_sec/cp_challenge/ak-challenge-3-1.htm"></form></body></html>"#,
            "https://www.example.com/",
        );

        assert!(surface.challenge_page);
        assert!(!surface.is_protected());
    }
}
