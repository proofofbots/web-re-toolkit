use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use url::Url;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tenant {
    pub origin: String,
    pub site: String,
    pub tenant: String,
}

impl Tenant {
    pub fn base(&self) -> String {
        format!("{}/{}/{}", self.origin, self.site, self.tenant)
    }

    pub fn endpoint(&self, name: &str) -> String {
        format!("{}/{name}", self.base())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Surface {
    pub tenant: Option<Tenant>,
    pub version: Option<String>,
    pub loaders: Vec<String>,
    pub script: Option<String>,
    pub configures: bool,
    pub mentions_sdk: bool,
    pub interstitial: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Challenge {
    pub im: String,
    pub stages: Vec<String>,
    pub script: Option<String>,
}

static LOADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})/([a-z0-9]+)\.js",
    )
    .expect("loader pattern")
});

static SCRIPT_SRC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<script[^>]+src="([^"]+ips\.js[^"]*)""#).expect("script pattern")
});

static SCRIPT_ANY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"src=["']([^"']*ips\.js[^"']*)["']"#).expect("script pattern"));

static MESSAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"postMessage\('KPSDK:MC:([^']+)'").expect("message pattern"));

static VERSION_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)x-kpsdk-v["'=:\s]+([a-z]-[\d.]+)"#).expect("version pattern")
});

static VERSION_POOL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"j-[\d.]+").expect("version pattern"));

const STAGE_NAMES: [&str; 4] = ["loadClient", "tiScripts", "interrogation", "encryptAndSend"];

pub fn surface(html: &str, page_url: &str) -> Surface {
    let origin = Url::parse(page_url)
        .ok()
        .map(|parsed| parsed.origin().ascii_serialization())
        .unwrap_or_default();

    let mut loaders: Vec<String> = Vec::new();
    let mut tenant = None;

    for found in LOADER.captures_iter(html) {
        let whole = found[0].to_string();
        if !loaders.contains(&whole) {
            loaders.push(whole);
        }

        if tenant.is_none() {
            tenant = Some(Tenant {
                origin: origin.clone(),
                site: found[1].to_string(),
                tenant: found[2].to_string(),
            });
        }
    }

    let script = SCRIPT_SRC
        .captures(html)
        .or_else(|| SCRIPT_ANY.captures(html))
        .map(|found| found[1].replace("&amp;", "&"))
        .and_then(|href| Url::parse(page_url).ok()?.join(&href).ok())
        .map(|url| url.to_string());

    Surface {
        tenant,
        version: VERSION_ATTR
            .captures(html)
            .map(|found| found[1].to_string()),
        loaders,
        interstitial: script.is_some(),
        script,
        configures: html.contains("KPSDK.configure")
            || html.contains("kpsdk-ready")
            || html.contains("kpsdk-load"),
        mentions_sdk: html.contains("KPSDK"),
    }
}

pub fn version_in_loader(source: &str) -> Option<String> {
    VERSION_POOL
        .find(source)
        .map(|found| found.as_str().to_string())
}

pub fn challenge(html: &str) -> Option<Challenge> {
    let message = MESSAGE.captures(html)?;
    let mut parts = message[1].split(':');
    let im = parts.next()?.to_string();

    Some(Challenge {
        im,
        stages: parts.map(str::to_string).collect(),
        script: SCRIPT_ANY
            .captures(html)
            .map(|found| found[1].replace("&amp;", "&")),
    })
}

pub fn stage_key(stages: &[String]) -> Option<String> {
    let mut key = String::new();

    for (index, encoded) in stages.iter().enumerate() {
        let Some(plain) = STAGE_NAMES.get(index) else {
            continue;
        };

        let Ok(bytes) = URL_SAFE_NO_PAD.decode(encoded.trim_end_matches('=')) else {
            continue;
        };

        if bytes.len() < plain.len() {
            continue;
        }

        let candidate: String = plain
            .bytes()
            .enumerate()
            .map(|(at, byte)| char::from(bytes[at] ^ byte))
            .collect();

        if candidate.len() > key.len() {
            key = candidate;
        }
    }

    (!key.is_empty()).then_some(key)
}

const COMPONENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

pub fn agent_url(tenant: &Tenant, version: &str, ct: &str, im: &str) -> String {
    let encode = |value: &str| percent_encoding::utf8_percent_encode(value, COMPONENT).to_string();

    format!(
        "{}?KP_UIDz={}&x-kpsdk-v={version}&x-kpsdk-im={}",
        tenant.endpoint("ips.js"),
        encode(ct),
        encode(im)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: &str = "149e9513-01fa-4fb0-aad4-566afd725d1b";
    const TENANT: &str = "2d206a39-8ed7-437e-a3be-862e0f06eea3";

    fn page() -> String {
        format!(
            r#"<html><head><script src="/{SITE}/{TENANT}/p.js"></script>
            <script src="/{SITE}/{TENANT}/ips.js?KP_UIDz=abc&amp;x-kpsdk-v=j-1.2.661"></script>
            <script>window.parent.postMessage('KPSDK:MC:token:AAAA:BBBB','*')</script>
            </head><body>KPSDK.configure([])</body></html>"#
        )
    }

    #[test]
    fn a_page_names_its_tenant_and_its_agent() {
        let found = surface(&page(), "https://www.example.com/buy");

        assert_eq!(
            found.tenant,
            Some(Tenant {
                origin: "https://www.example.com".to_string(),
                site: SITE.to_string(),
                tenant: TENANT.to_string(),
            })
        );
        assert!(found.configures);
        assert!(found.mentions_sdk);
        assert!(found.interstitial);
        assert_eq!(
            found.script.as_deref(),
            Some(
                "https://www.example.com/149e9513-01fa-4fb0-aad4-566afd725d1b/2d206a39-8ed7-437e-a3be-862e0f06eea3/ips.js?KP_UIDz=abc&x-kpsdk-v=j-1.2.661"
            )
        );
    }

    #[test]
    fn a_page_with_no_kasada_reports_nothing() {
        let found = surface("<html><body>hello</body></html>", "https://plain.example/");
        assert!(found.tenant.is_none());
        assert!(!found.mentions_sdk);
        assert!(!found.interstitial);
    }

    #[test]
    fn the_challenge_message_splits_into_a_token_and_its_stages() {
        let parsed = challenge(&page()).expect("no message");
        assert_eq!(parsed.im, "token");
        assert_eq!(parsed.stages, vec!["AAAA".to_string(), "BBBB".to_string()]);
    }

    #[test]
    fn the_stage_names_hand_the_repeating_key_over() {
        let key = "secretkey";
        let stages: Vec<String> = STAGE_NAMES
            .iter()
            .map(|plain| {
                let bytes: Vec<u8> = plain
                    .bytes()
                    .enumerate()
                    .map(|(index, byte)| byte ^ key.as_bytes()[index % key.len()])
                    .collect();
                URL_SAFE_NO_PAD.encode(bytes)
            })
            .collect();

        let recovered = stage_key(&stages).expect("no key");
        assert_eq!(&recovered[..key.len()], key);
    }

    #[test]
    fn the_agent_url_carries_the_token_and_the_build() {
        let tenant = Tenant {
            origin: "https://www.example.com".to_string(),
            site: SITE.to_string(),
            tenant: TENANT.to_string(),
        };

        let url = agent_url(&tenant, "j-1.2.661", "a b", "im");
        assert!(url.contains("/ips.js?KP_UIDz=a%20b"));
        assert!(url.ends_with("&x-kpsdk-v=j-1.2.661&x-kpsdk-im=im"));
    }
}
