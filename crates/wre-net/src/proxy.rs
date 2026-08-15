use std::fmt;

use rand::RngExt;
use serde::{Deserialize, Serialize};

use wre_core::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxySpec {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub scheme: ProxyScheme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyScheme {
    #[default]
    Socks5,
    Http,
    Https,
}

impl ProxyScheme {
    pub fn prefix(self) -> &'static str {
        match self {
            ProxyScheme::Socks5 => "socks5h",
            ProxyScheme::Http => "http",
            ProxyScheme::Https => "https",
        }
    }
}

impl ProxySpec {
    pub fn parse(spec: &str) -> Result<Self> {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return Err(Error::msg("empty proxy spec"));
        }

        let (scheme, rest) = match trimmed.split_once("://") {
            Some(("socks5" | "socks5h" | "socks", rest)) => (ProxyScheme::Socks5, rest),
            Some(("http", rest)) => (ProxyScheme::Http, rest),
            Some(("https", rest)) => (ProxyScheme::Https, rest),
            Some((other, _)) => return Err(Error::msg(format!("unknown proxy scheme {other}"))),
            None => (ProxyScheme::Socks5, trimmed),
        };

        if let Some((credentials, endpoint)) = rest.rsplit_once('@') {
            let (user, password) = credentials
                .split_once(':')
                .map(|(u, p)| (u.to_string(), p.to_string()))
                .unwrap_or_else(|| (credentials.to_string(), String::new()));
            let (host, port) = split_endpoint(endpoint)?;
            return Ok(Self {
                host,
                port,
                user: Some(user),
                password: if password.is_empty() { None } else { Some(password) },
                scheme,
            });
        }

        let parts: Vec<&str> = rest.splitn(4, ':').collect();
        match parts.as_slice() {
            [host, port] => Ok(Self {
                host: (*host).to_string(),
                port: parse_port(port)?,
                user: None,
                password: None,
                scheme,
            }),
            [host, port, user] => Ok(Self {
                host: (*host).to_string(),
                port: parse_port(port)?,
                user: Some((*user).to_string()),
                password: None,
                scheme,
            }),
            [host, port, user, password] => Ok(Self {
                host: (*host).to_string(),
                port: parse_port(port)?,
                user: Some((*user).to_string()),
                password: Some((*password).to_string()),
                scheme,
            }),
            _ => Err(Error::msg(format!(
                "proxy spec should be host:port[:user[:pass]], got {spec}"
            ))),
        }
    }

    pub fn from_env() -> Option<Self> {
        std::env::var("WRE_PROXY")
            .ok()
            .and_then(|spec| ProxySpec::parse(&spec).ok())
    }

    pub fn url(&self) -> String {
        match (&self.user, &self.password) {
            (Some(user), Some(password)) => format!(
                "{}://{}:{}@{}:{}",
                self.scheme.prefix(),
                percent(user),
                percent(password),
                self.host,
                self.port
            ),
            (Some(user), None) => format!(
                "{}://{}@{}:{}",
                self.scheme.prefix(),
                percent(user),
                self.host,
                self.port
            ),
            _ => format!("{}://{}:{}", self.scheme.prefix(), self.host, self.port),
        }
    }

    pub fn session(&self) -> Option<String> {
        let password = self.password.as_ref()?;
        capture_tag(password, "session")
    }

    pub fn country(&self) -> Option<String> {
        let password = self.password.as_ref()?;
        capture_tag(password, "country")
    }

    pub fn with_session(&self, session: Option<&str>) -> Self {
        let session = session
            .map(str::to_string)
            .unwrap_or_else(|| random_session(10));

        let mut clone = self.clone();
        if let Some(password) = clone.password.clone() {
            clone.password = Some(replace_tag(&password, "session", &session));
        }
        clone
    }

    pub fn with_country(&self, country: &str) -> Self {
        let mut clone = self.clone();
        if let Some(password) = clone.password.clone() {
            clone.password = Some(replace_tag(&password, "country", country));
        }
        clone
    }

    pub fn rotated(&self) -> Self {
        self.with_session(None)
    }
}

impl fmt::Display for ProxySpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}://{}:{}", self.scheme.prefix(), self.host, self.port)
    }
}

fn split_endpoint(endpoint: &str) -> Result<(String, u16)> {
    let (host, port) = endpoint
        .rsplit_once(':')
        .ok_or_else(|| Error::msg(format!("proxy endpoint missing port: {endpoint}")))?;
    Ok((host.to_string(), parse_port(port)?))
}

fn parse_port(port: &str) -> Result<u16> {
    port.parse::<u16>()
        .map_err(|_| Error::msg(format!("bad proxy port {port}")))
}

fn percent(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn capture_tag(text: &str, tag: &str) -> Option<String> {
    let needle = format!("{tag}-");
    let start = text.find(&needle)? + needle.len();
    let rest = &text[start..];
    let end = rest
        .find(|ch: char| !ch.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    if end == 0 { None } else { Some(rest[..end].to_string()) }
}

fn replace_tag(text: &str, tag: &str, value: &str) -> String {
    let needle = format!("{tag}-");

    if let Some(start) = text.find(&needle) {
        let after = start + needle.len();
        let rest = &text[after..];
        let end = rest
            .find(|ch: char| !ch.is_ascii_alphanumeric())
            .unwrap_or(rest.len());
        return format!("{}{}{}", &text[..after], value, &rest[end..]);
    }

    let separator = if text.contains('_') { "_" } else { "-" };
    format!("{text}{separator}{tag}-{value}")
}

pub fn random_session(length: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    (0..length)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colon_form() {
        let spec = ProxySpec::parse("gate.example.com:7000:user:pass_country-us_session-ABC").unwrap();
        assert_eq!(spec.host, "gate.example.com");
        assert_eq!(spec.port, 7000);
        assert_eq!(spec.session().as_deref(), Some("ABC"));
        assert_eq!(spec.country().as_deref(), Some("us"));
    }

    #[test]
    fn parses_url_form() {
        let spec = ProxySpec::parse("socks5://user:pass@gate.example.com:7000").unwrap();
        assert_eq!(spec.user.as_deref(), Some("user"));
        assert_eq!(spec.scheme, ProxyScheme::Socks5);
    }

    #[test]
    fn rotates_session_in_place() {
        let spec = ProxySpec::parse("h:1:u:base_session-OLD_country-de").unwrap();
        let rotated = spec.with_session(Some("NEW"));
        assert_eq!(rotated.password.as_deref(), Some("base_session-NEW_country-de"));
        assert_eq!(rotated.country().as_deref(), Some("de"));
    }

    #[test]
    fn adds_session_when_missing() {
        let spec = ProxySpec::parse("h:1:u:base").unwrap();
        let rotated = spec.with_session(Some("NEW"));
        assert_eq!(rotated.password.as_deref(), Some("base-session-NEW"));
    }
}
